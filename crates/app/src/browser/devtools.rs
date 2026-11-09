use std::time::{Duration, Instant};
use serde_json::json;
use tao::platform::windows::WindowExtWindows;
use wry::{Rect, dpi::{PhysicalPosition, PhysicalSize}};
use super::Browser;

/// DevTools opens on Elements, without Microsoft's Welcome tab ("Welcome to Microsoft Edge
/// DevTools") or its after-update page. Seeded into the WebView2 profile's preferences before
/// its browser process starts; a file that cannot be read is left alone.
pub(crate) fn quiet_devtools_welcome(data_dir: &std::path::Path) {
    let path = data_dir.join("EBWebView").join("Default").join("Preferences");
    let mut prefs = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(v) if v.is_object() => v,
            _ => return,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return,
    };
    if !quiet_welcome(&mut prefs) {
        return;
    }
    let (Some(dir), Ok(text)) = (path.parent(), serde_json::to_vec(&prefs)) else { return };
    let tmp = path.with_extension("kakangkung-tmp");
    let written = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&tmp, text)).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        eprintln!("DevTools preferences: {e}");
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Close the Welcome tab and turn its page off in WebView2 `prefs`; true when anything changed.
/// (DevTools keeps each preference as a JSON-encoded string.)
fn quiet_welcome(prefs: &mut serde_json::Value) -> bool {
    let Some(root) = prefs.as_object_mut() else { return false };
    let devtools = root.entry("devtools").or_insert_with(|| json!({}));
    if !devtools.is_object() { *devtools = json!({}); }
    let p = devtools.as_object_mut().and_then(|d| d.entry("preferences").or_insert_with(|| json!({})).as_object_mut());
    let Some(p) = p else { return false };
    let before = p.clone();
    p.insert("help.show-release-note".into(), json!("false"));
    let mut tabs = p.get("closeable-tabs").and_then(|v| v.as_str()).and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    tabs["welcome"] = json!(false);
    p.insert("closeable-tabs".into(), json!(tabs.to_string()));
    if p.get("panel-selected-tab").and_then(|v| v.as_str()).is_none_or(|s| s.contains("welcome")) {
        p.insert("panel-selected-tab".into(), json!("\"elements\""));
    }
    *p != before
}

impl Browser {
    /// Typing in the address bar with no web page open yet (the native New Tab page): start
    /// WebView2 now, so the page the person is about to open does not wait about two seconds
    /// for its browser process. Not at start-up, which would cost memory before any browsing.
    pub(crate) fn prewarm_webview2(&mut self) {
        if self.env.is_none() {
            if let Some(dir) = self.ctx.data_directory() {
                platform::prewarm_environment(dir, &self.browser_args);
            }
        }
    }

    pub(crate) fn close_devtools(&mut self) {
        if self.devtools.take().is_some() {
            self.devtools_ready = false;
            if let Some(dock) = &self.devtools_dock { dock.hide(); }
            self.relayout();
        }
    }

    pub(crate) fn toggle_devtools(&mut self) {
        if self.devtools.is_some() { return self.close_devtools(); }
        let Some(view) = self.active_webview() else { return };
        match platform::DockedDevTools::open(view, self.window.hwnd()) {
            Ok(panel) => {
                self.devtools = Some(panel);
                self.devtools_tab = self.active;
                self.devtools_ready = false;
                self.relayout();
            }
            Err(error) => self.devtools_error(&error),
        }
    }

    fn devtools_error(&mut self, error: &str) {
        eprintln!("DevTools panel: {error}");
        self.show_overlay("devtools-error", [12.0, 94.0, 520.0, 0.0],
            &json!({"rows":[{"header":true,"label":"Developer tools"},{"note":true,"label":error}]}).to_string());
    }

    /// The page area (physical x, y, width, height).
    fn page_area(&self) -> [i32; 4] {
        let scale = self.window.scale_factor();
        let page = super::page_bounds(&self.window);
        let (p, s) = (page.position.to_physical::<i32>(scale), page.size.to_physical::<u32>(scale));
        [p.x, p.y, s.width as i32, s.height as i32]
    }

    /// Where things go with DevTools docked at `devtools_side`: (pages, DevTools, its button
    /// rail, the splitter between them), physical client rectangles.
    fn dock_layout(&self) -> [[i32; 4]; 4] {
        let [x, y, w, h] = self.page_area();
        if self.devtools.is_none() {
            return [[x, y, w, h], [0; 4], [0; 4], [0; 4]];
        }
        let scale = self.window.scale_factor();
        let px = |dips: f64| (dips * scale).round() as i32;
        let (rail, split) = (px(platform::DOCK_RAIL_W), px(platform::DOCK_SPLIT_W).max(3));
        let share = self.devtools_share.clamp(0.15, 0.85);
        // The panel keeps a usable size, and so do the pages.
        let size = |whole: i32, least_panel: i32, least_page: i32| ((whole as f32 * share) as i32).min(whole - least_page - split).max(least_panel.min(whole)).max(0);
        match self.devtools_side {
            "right" => {
                let dw = size(w, px(260.0), px(200.0));
                let left = x + w - dw;
                [[x, y, (w - dw - split).max(0), h], [left, y, (dw - rail).max(1), h], [x + w - rail, y, rail, h], [left - split, y, split, h]]
            }
            "left" => {
                let dw = size(w, px(260.0), px(200.0));
                [[x + dw + split, y, (w - dw - split).max(0), h], [x + rail, y, (dw - rail).max(1), h], [x, y, rail, h], [x + dw, y, split, h]]
            }
            _ => {
                let dh = size(h, px(120.0), px(80.0));
                let top = y + h - dh;
                [[x, y, w, (h - dh - split).max(0)], [x, top, (w - rail).max(1), dh], [x + w - rail, top, rail, dh], [x, top - split, w, split]]
            }
        }
    }

    pub(crate) fn content_bounds(&self) -> Rect {
        let [x, y, w, h] = self.dock_layout()[0];
        Rect { position: PhysicalPosition::new(x, y).into(), size: PhysicalSize::new(w.max(0) as u32, h.max(0) as u32).into() }
    }

    pub(crate) fn devtools_bounds(&self) -> [i32; 4] {
        self.dock_layout()[1]
    }

    /// Size DevTools and place its rail and splitter (hidden while it is closed).
    pub(crate) fn place_devtools(&mut self) {
        let [_, panel, rail, splitter] = self.dock_layout();
        let Some(devtools) = self.devtools.as_mut() else {
            if let Some(dock) = &self.devtools_dock { dock.hide(); }
            return;
        };
        let _ = devtools.resize(panel);
        if self.devtools_dock.is_none() {
            let proxy = self.proxy.clone();
            self.devtools_dock = platform::DockControls::create(self.window.hwnd(), move |cmd| {
                let _ = proxy.send_event(crate::msg::Msg::Ipc(cmd));
            }).ok();
        }
        if let Some(dock) = &self.devtools_dock { dock.place(rail, splitter, self.devtools_side, self.dark_ui()); }
    }

    /// Keep the dock's side and size for the next time the browser starts.
    fn save_dock(&mut self) {
        self.settings.devtools_dock = self.devtools_side;
        self.settings.devtools_share = self.devtools_share;
        let (side, share) = (self.devtools_side, format!("{:.3}", self.devtools_share));
        self.db(move |s| s.set_settings(&[(crate::prefs::DEVTOOLS_DOCK, side), (crate::prefs::DEVTOOLS_SHARE, &share)]));
    }

    /// The dock's rail and splitter: `dockmenu\t<x>\t<y>` (⋮, its menu's corner on screen),
    /// `dockclose`, `docksplit\t<x>\t<y>` while the splitter is dragged (pointer, physical client
    /// pixels) and `dockdone` when it is let go.
    pub(crate) fn dock_command(&mut self, cmd: &str, a: &str, b: &str) {
        match cmd {
            "dockdone" => return self.save_dock(),
            // The rail's ⋮: where to dock, and close (a, b: the menu's corner on screen).
            "dockmenu" => {
                use platform::menu::MenuItem;
                let tr = |s: &str| crate::i18n::tr(s).into_owned();
                let side = |id: u32, label: &str, at: &str| MenuItem::Item { id, label: tr(label), shortcut: String::new(), enabled: true, checked: self.devtools_side == at };
                let items = [side(1, "Dock to bottom", "bottom"), side(2, "Dock to right", "right"), side(3, "Dock to left", "left"), MenuItem::Separator, MenuItem::new(4, &tr("Close DevTools"), "")];
                let (x, y) = (a.parse().unwrap_or(0), b.parse().unwrap_or(0));
                match platform::menu::popup(self.window.hwnd(), x, y, self.devtools_side != "left", &items) {
                    Some(4) => return self.close_devtools(),
                    Some(n @ 1..=3) => {
                        self.devtools_side = ["bottom", "right", "left"][n as usize - 1];
                        self.save_dock();
                    }
                    _ => return,
                }
            }
            "dockclose" => return self.close_devtools(),
            "docksplit" => {
                let [x, y, w, h] = self.page_area();
                let (px, py) = (a.parse::<i32>().unwrap_or(0), b.parse::<i32>().unwrap_or(0));
                let share = match self.devtools_side {
                    "right" => (x + w - px) as f32 / w.max(1) as f32,
                    "left" => (px - x) as f32 / w.max(1) as f32,
                    _ => (y + h - py) as f32 / h.max(1) as f32,
                };
                self.devtools_share = share.clamp(0.15, 0.85);
            }
            _ => return,
        }
        if self.devtools.is_some() { self.relayout(); }
    }

    pub(crate) fn devtools_tick(&mut self) -> Option<Instant> {
        let bounds = self.devtools_bounds();
        let result = self.devtools.as_mut()?.poll(bounds);
        match result {
            Ok(ready) => self.devtools_ready = ready,
            Err(error) => {
                self.close_devtools();
                self.devtools_error(&error);
                return None;
            }
        }
        Some(Instant::now() + Duration::from_millis(if self.devtools_ready { 250 } else { 16 }))
    }
}

#[cfg(test)]
mod tests {
    use super::quiet_welcome;
    use serde_json::json;

    #[test]
    fn devtools_welcome_is_closed_and_choices_kept() {
        // A fresh profile gets every setting.
        let mut fresh = json!({});
        assert!(quiet_welcome(&mut fresh));
        let p = &fresh["devtools"]["preferences"];
        assert_eq!(p["help.show-release-note"], "false");
        assert_eq!(p["panel-selected-tab"], "\"elements\"");
        assert_eq!(serde_json::from_str::<serde_json::Value>(p["closeable-tabs"].as_str().unwrap()).unwrap()["welcome"], false);
        assert!(!quiet_welcome(&mut fresh), "nothing left to change the second time");
        // An existing profile: other open tabs and the chosen panel stay; Welcome closes.
        let mut used = json!({"other": 1, "devtools": {"preferences": {"closeable-tabs": "{\"network\":true,\"welcome\":true}", "panel-selected-tab": "\"console\""}}});
        assert!(quiet_welcome(&mut used));
        let p = &used["devtools"]["preferences"];
        assert_eq!(p["panel-selected-tab"], "\"console\"");
        let tabs: serde_json::Value = serde_json::from_str(p["closeable-tabs"].as_str().unwrap()).unwrap();
        assert_eq!((tabs["network"].clone(), tabs["welcome"].clone()), (json!(true), json!(false)));
        assert_eq!(used["other"], 1);
    }
}
