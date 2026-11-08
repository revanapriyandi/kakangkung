//! Extensions in the browser window: installs from the Chrome Web Store (the store page's button
//! talks to `store_message`), the toolbar's extensions menu, and an extension's toolbar pop-up
//! shown under that button (WebView2 has no extension toolbar of its own).

use serde_json::json;
use crate::i18n::tr;
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
                // The page's button is ours, but the page could click it: like Chrome, the
                // extension is fetched first and only added once the person has seen what it
                // can do and agreed (`ask_install`). One at a time.
                if self.pending_install.is_some() {
                    return;
                }
                let name: String = name.chars().take(80).collect();
                self.pending_install = Some(PendingInstall { tab, ext: ext.clone(), name, folder: None });
                self.store_button(tab, &ext, "busy", "Checking…");
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
                self.pending_install = None;
                return self.store_button(tab, &ext, "ready", &failed(&e));
            }
        };
        match self.pending_install.as_mut() {
            Some(p) if p.ext == ext => p.folder = Some(folder),
            _ => return,
        }
        self.ask_install();
    }

    /// Chrome's install dialog under the toolbar: 'Add "name"?', what the extension can do,
    /// Add extension / Cancel. The answer comes back as a pick of the "extinstall" pop-up.
    fn ask_install(&mut self) {
        let Some(p) = &self.pending_install else { return };
        let Some(folder) = p.folder.clone() else { return };
        let (tab, ext) = (p.tab, p.ext.clone());
        let title = if p.name.is_empty() { tr("Add this extension?").into_owned() } else { tr("Add \"{name}\"?").replace("{name}", &p.name) };
        self.store_button(tab, &ext, "busy", "Waiting for you…");
        let mut rows = vec![json!({ "header": true, "id": "", "label": title })];
        let warnings = webstore::permission_warnings(&folder);
        if warnings.is_empty() {
            rows.push(json!({ "note": true, "label": tr("It needs no special permissions.") }));
        } else {
            rows.push(json!({ "note": true, "label": tr("It can:") }));
            rows.extend(warnings.iter().map(|w| json!({ "note": true, "label": format!("•  {}", tr(w)) })));
        }
        rows.push(json!({ "buttons": true, "id": "add", "label": tr("Add extension"), "key": tr("Cancel"), "close": "cancel" }));
        let scale = self.window.scale_factor();
        let width = self.window.inner_size().width as f64 / scale;
        let w = 440.0;
        let data = json!({ "rows": rows }).to_string();
        self.show_overlay("extinstall", [(width - w - 8.0).max(0.0), platform::frame::FRAME_H as f64 - 4.0, w, 0.0], &data);
    }

    /// The install dialog was answered (or closed: `add` false).
    pub(crate) fn finish_install(&mut self, add: bool) {
        let Some(PendingInstall { tab, ext, folder, .. }) = self.pending_install.take() else { return };
        self.hide_overlay();
        let Some(folder) = folder else { return };
        if !add {
            let _ = std::fs::remove_dir_all(&folder);
            return self.store_button(tab, &ext, "ready", "Add to Kakangkung");
        }
        self.store_button(tab, &ext, "busy", "Adding…");
        self.load_store_extension(tab, ext, folder);
    }

    fn load_store_extension(&mut self, tab: u64, ext: String, folder: std::path::PathBuf) {
        let failed = |e: &str| format!("Couldn't add: {}", e.chars().take(90).collect::<String>());
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
        } else {
            // Chrome's pin buttons: a checked item per extension.
            let pins: Vec<serde_json::Value> = list
                .iter()
                .filter(|e| e.enabled && !super::api::BUILTIN_EXTENSIONS.contains(&e.id.as_str()))
                .map(|e| json!({ "id": format!("extpin:{}", e.id), "label": e.name, "key": "", "checked": self.settings.pinned_extensions.contains(&e.id) }))
                .collect();
            rows.push(json!({ "sep": true }));
            rows.push(json!({ "label": "Pin to toolbar", "sub": pins }));
        }
        rows.push(json!({ "sep": true }));
        rows.push(json!({ "id": "extstore", "label": "Find extensions in the Chrome Web Store", "key": "" }));
        rows.push(json!({ "id": "extensions", "label": "Manage extensions", "key": "" }));
        // The choice comes back through `on_menu` (extbubble:, extpin:, extstore, extensions).
        self.menu_at = (right, top);
        self.native_popup(rows, right, top, true);
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

    /// Pin extension `id` to the toolbar, or unpin it; kept in the `PINNED_EXTENSIONS` setting.
    pub(crate) fn toggle_pin(&mut self, id: &str) {
        let pins = &mut self.settings.pinned_extensions;
        match pins.iter().position(|p| p == id) {
            Some(i) => {
                pins.remove(i);
            }
            None if crate::webstore::valid_id(id) => pins.push(id.to_string()),
            None => return,
        }
        let value = pins.join(",");
        self.db(move |s| {
            s.set_setting(crate::prefs::PINNED_EXTENSIONS, &value)?;
            crate::profile::touch_settings(s);
            Ok(())
        });
        self.render();
    }

    /// The toolbar's pinned buttons: built-in pages, then extensions (not in Incognito, where
    /// extensions do not run). An extension's icon comes from its manifest, once per extension.
    pub(crate) fn pinned_buttons(&self) -> Vec<platform::frame::Pinned> {
        let pages = self.settings.pinned_pages.iter().map(|page| platform::frame::Pinned {
            id: format!("page:{page}"),
            glyph: crate::prefs::PINNABLE_PAGES.iter().find(|(p, _)| p == page).map_or("", |(_, g)| *g),
            icon: String::new(),
        });
        let mut out: Vec<platform::frame::Pinned> = pages.collect();
        if self.incognito {
            return out;
        }
        for id in self.settings.pinned_extensions.clone() {
            let icon = self
                .pinned_icons
                .borrow_mut()
                .entry(id.clone())
                .or_insert_with(|| crate::webstore::button_icon(&self.paths.extensions.join(&id)).and_then(|f| platform::load_icon_file(&f)).unwrap_or_default())
                .clone();
            out.push(platform::frame::Pinned { id: format!("ext:{id}"), glyph: "\u{EA86}", icon });
        }
        out
    }

    /// A pinned toolbar button was clicked; (`right`, `top`) is its lower right corner.
    pub(crate) fn open_pinned(&mut self, id: &str, right: f64, top: f64) {
        if let Some(ext) = id.strip_prefix("ext:") {
            self.open_bubble(ext, right, top);
        } else if let Some(page) = id.strip_prefix("page:") {
            self.open_after_active(&format!("{}{page}", ui::INTERNAL));
        }
    }

    /// Right-click on a pinned button: its menu (Unpin, through `on_menu`).
    pub(crate) fn pinned_menu(&mut self, id: &str, x: f64, y: f64) {
        let rows = vec![json!({ "id": format!("unpin:{id}"), "label": "Unpin from toolbar", "key": "" })];
        self.native_popup(rows, x, y, false);
    }

    /// Unpin toolbar button `id` (`ext:<id>` or `page:<name>`).
    pub(crate) fn unpin(&mut self, id: &str) {
        if let Some(ext) = id.strip_prefix("ext:") {
            self.toggle_pin(ext);
        } else if let Some(page) = id.strip_prefix("page:") {
            let key = format!("appearance.{page}_button");
            self.settings.pinned_pages.retain(|p| *p != page);
            self.db(move |s| {
                s.set_setting(&key, "0")?;
                crate::profile::touch_settings(s);
                Ok(())
            });
            self.render();
        }
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

/// An extension from the Chrome Web Store waiting for the person's answer: fetched into
/// `folder` (once downloaded) but not loaded.
pub(crate) struct PendingInstall {
    pub(crate) tab: u64,
    pub(crate) ext: String,
    pub(crate) name: String,
    pub(crate) folder: Option<std::path::PathBuf>,
}
