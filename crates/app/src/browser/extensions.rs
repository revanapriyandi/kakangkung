//! Extensions in the browser window: installs from the Chrome Web Store (the store page's button
//! talks to `store_message`), the toolbar's extensions menu, and an extension's toolbar pop-up
//! shown under that button (WebView2 has no extension toolbar of its own).

use serde_json::json;
use tao::platform::windows::WindowExtWindows;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebViewBuilder, WebViewBuilderExtWindows};

use crate::browser::Browser;
use crate::msg::Msg;
use crate::webstore;

/// Extension pop-up size limits in DIPs (Chrome: 25×25 up to 800×600).
const BUBBLE_MIN: (f64, f64) = (25.0, 25.0);
const BUBBLE_MAX: (f64, f64) = (800.0, 600.0);
/// Size before the pop-up has reported its content (estimated, a typical pop-up).
const BUBBLE_START: (f64, f64) = (360.0, 480.0);
/// Browser version asked of the store when the runtime does not say (a recent Chromium).
const FALLBACK_VERSION: &str = "150.0.0.0";

/// Runs in the extension pop-up: reports the size its content wants, so the pop-up fits it.
const BUBBLE_SIZE_JS: &str = r#"(() => {
  const send = () => {
    const b = document.body;
    if (!b || !window.ipc) return;
    const r = b.getBoundingClientRect(), s = getComputedStyle(b);
    const w = r.width + parseFloat(s.marginLeft) + parseFloat(s.marginRight);
    const h = Math.max(b.scrollHeight, r.height) + parseFloat(s.marginTop) + parseFloat(s.marginBottom);
    window.ipc.postMessage("kk:size:" + Math.ceil(w) + "," + Math.ceil(h));
  };
  addEventListener("load", () => { send(); new ResizeObserver(send).observe(document.body); });
})();"#;

impl Browser {
    /// Show `text` on the store page's button for `ext` in tab `tab`.
    pub(crate) fn store_button(&self, tab: u64, ext: &str, state: &str, text: &str) {
        if let Some(wv) = self.index(tab).and_then(|i| self.tabs[i].webview.as_ref()) {
            let _ = wv.evaluate_script(&format!("window.__kkStore && window.__kkStore({}, {}, {})", json!(ext), json!(state), json!(text)));
        }
    }

    fn runtime_version(&self) -> String {
        self.extension_host().and_then(|wv| platform::runtime_version(wv).ok()).filter(|v| !v.is_empty()).unwrap_or_else(|| FALLBACK_VERSION.into())
    }

    /// A message from the Chrome Web Store page in tab `tab`: "check\t<id>" (is it installed?)
    /// or "add\t<id>\t<name>" (the button was clicked).
    pub(crate) fn store_message(&mut self, tab: u64, cmd: &str) {
        let mut f = cmd.splitn(3, '\t');
        let (what, ext, name) = (f.next().unwrap_or(""), f.next().unwrap_or("").to_string(), f.next().unwrap_or("").trim().to_string());
        if !webstore::valid_id(&ext) {
            return;
        }
        let proxy = self.proxy.clone();
        match what {
            "check" => match self.extension_host() {
                Some(wv) => platform::extensions_list(wv, move |r| {
                    let installed = r.is_ok_and(|v| v.iter().any(|e| e.id == ext));
                    let (state, text) = if installed { ("done", "Added to Kakangkung") } else { ("ready", "Add to Kakangkung") };
                    let _ = proxy.send_event(Msg::StoreButton { tab, ext, state, text: text.into() });
                }),
                None => self.store_button(tab, &ext, "ready", "Add to Kakangkung"),
            },
            "add" => {
                // The page's button is ours, but the page could click it: the person decides here.
                let what = if name.is_empty() { "this extension".to_string() } else { format!("\"{}\"", name.chars().take(80).collect::<String>()) };
                let ask = format!(
                    "Add {what} to Kakangkung?\n\nIt is downloaded from the Chrome Web Store. Depending on what it asks for, it can read and change your data on websites."
                );
                if !platform::confirm(self.window.hwnd(), "Add extension", &ask) {
                    return;
                }
                self.store_button(tab, &ext, "busy", "Adding…");
                let (root, version) = (self.paths.extensions.clone(), self.runtime_version());
                std::thread::spawn(move || {
                    let result = webstore::install(&ext, &version, &root);
                    let _ = proxy.send_event(Msg::StoreUnpacked { tab, ext, result });
                });
            }
            _ => {}
        }
    }

    /// A store extension is on disk (or failed to get there): load it into the tabs' profile.
    pub(crate) fn store_unpacked(&mut self, tab: u64, ext: String, result: Result<std::path::PathBuf, String>) {
        let failed = |e: &str| format!("Couldn't add: {}", e.chars().take(90).collect::<String>());
        let folder = match result {
            Ok(f) => f,
            Err(e) => {
                eprintln!("web store install {ext}: {e}");
                return self.store_button(tab, &ext, "ready", &failed(&e));
            }
        };
        let Some(wv) = self.extension_host() else { return self.store_button(tab, &ext, "ready", &failed("open a tab first")) };
        let proxy = self.proxy.clone();
        platform::extension_add(wv, &folder, move |r| {
            let (state, text) = match r {
                Ok(_) => ("done", "Added to Kakangkung".to_string()),
                Err(e) => {
                    eprintln!("web store install {ext}: {e}");
                    ("ready", failed(&e))
                }
            };
            let _ = proxy.send_event(Msg::StoreButton { tab, ext, state, text });
        });
    }

    /// The extensions button was pressed at (`right`, `top`): list the extensions, then show
    /// the menu (`Msg::ExtMenu`).
    pub(crate) fn request_extensions_menu(&mut self, right: f64, top: f64) {
        let proxy = self.proxy.clone();
        match self.extension_host() {
            Some(wv) => platform::extensions_list(wv, move |r| {
                let _ = proxy.send_event(Msg::ExtMenu { list: r.unwrap_or_default(), x: right, y: top });
            }),
            None => self.show_extensions_menu(vec![], right, top),
        }
    }

    /// Chrome's extensions menu: each extension (click = its toolbar pop-up), then Manage.
    pub(crate) fn show_extensions_menu(&mut self, list: Vec<platform::Extension>, right: f64, top: f64) {
        let mut rows: Vec<serde_json::Value> = list
            .iter()
            .filter(|e| e.enabled && !super::api::BUILTIN_EXTENSIONS.contains(&e.id.as_str()))
            .map(|e| json!({ "id": format!("extbubble:{}", e.id), "label": e.name, "key": "" }))
            .collect();
        if rows.is_empty() {
            rows.push(json!({ "id": "", "label": "No extensions yet", "key": "" }));
        }
        rows.push(json!({ "sep": true }));
        rows.push(json!({ "id": "extstore", "label": "Find extensions in the Chrome Web Store", "key": "" }));
        rows.push(json!({ "id": "extensions", "label": "Manage extensions", "key": "" }));
        let Some(pick) = self.native_popup(rows, right, top, true) else { return };
        if let Some(id) = pick.strip_prefix("extbubble:") {
            self.open_bubble(id, right, top);
        } else if pick == "extstore" {
            self.open_tab(&format!("https://{}/category/extensions", webstore::HOST), None);
        } else {
            self.on_menu(&pick);
        }
    }

    /// Show extension `id`'s toolbar pop-up under the button at (`right`, `top`) DIPs; one
    /// without a pop-up opens its options page in a tab, one Kakangkung cannot read (loaded from
    /// elsewhere) opens the Extensions page.
    /// Limit: the pop-up is its own webview, so an extension asking for "the active tab" gets
    /// the pop-up, not the page behind it (WebView2 has no API to tell it otherwise).
    pub(crate) fn open_bubble(&mut self, id: &str, right: f64, top: f64) {
        self.close_bubble();
        let Some((page, is_popup)) = webstore::button_page(&self.paths.extensions.join(id)) else {
            return self.open_after_active(&format!("{}extensions", ui::INTERNAL));
        };
        let url = format!("chrome-extension://{id}/{page}");
        let Some(env) = self.env.clone().filter(|_| is_popup) else { return self.open_tab(&url, None) };
        let (w, h) = BUBBLE_START;
        let p = self.proxy.clone();
        let built = WebViewBuilder::new()
            .with_environment(env)
            .with_url(&url)
            .with_initialization_script(BUBBLE_SIZE_JS)
            .with_ipc_handler(move |req| {
                let size = req.body().strip_prefix("kk:size:").and_then(|s| s.split_once(',')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                if size.is_some() {
                    let _ = p.send_event(Msg::Bubble { close: false, size });
                }
            })
            .with_bounds(Rect { position: LogicalPosition::new(right - w, top).into(), size: LogicalSize::new(w, h).into() })
            // Chrome shows pop-ups on white unless the extension paints its own background.
            .with_background_color((255, 255, 255, 255))
            .with_theme(self.settings.wry_theme())
            .build_as_child(&self.window);
        let wv = match built {
            Ok(wv) => wv,
            Err(e) => {
                eprintln!("extension pop-up {id}: {e}");
                return;
            }
        };
        let (p1, p2) = (self.proxy.clone(), self.proxy.clone());
        let _ = platform::on_lost_focus(&wv, move || {
            let _ = p1.send_event(Msg::Bubble { close: true, size: None });
        });
        let _ = platform::on_close_requested(&wv, move || {
            let _ = p2.send_event(Msg::Bubble { close: true, size: None });
        });
        let _ = wv.focus();
        self.bubble_at = (right, top);
        self.ext_bubble = Some(wv);
    }

    pub(crate) fn close_bubble(&mut self) {
        self.ext_bubble = None;
    }

    /// The pop-up asked to close, lost focus, or reported the size its content wants.
    pub(crate) fn on_bubble(&mut self, close: bool, size: Option<(f64, f64)>) {
        if close {
            return self.close_bubble();
        }
        let (Some(wv), Some((w, h))) = (&self.ext_bubble, size) else { return };
        let (w, h) = (w.clamp(BUBBLE_MIN.0, BUBBLE_MAX.0), h.clamp(BUBBLE_MIN.1, BUBBLE_MAX.1));
        let (right, top) = self.bubble_at;
        let _ = wv.set_bounds(Rect { position: LogicalPosition::new((right - w).max(0.0), top).into(), size: LogicalSize::new(w, h).into() });
    }
}
