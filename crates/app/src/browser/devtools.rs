use std::time::{Duration, Instant};
use serde_json::json;
use tao::platform::windows::WindowExtWindows;
use wry::{Rect, dpi::PhysicalSize};
use super::Browser;

impl Browser {
    pub(crate) fn close_devtools(&mut self) {
        if self.devtools.take().is_some() {
            self.devtools_ready = false;
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

    fn dock_height(&self) -> u32 {
        let full = super::page_bounds(&self.window).size.to_physical::<u32>(self.window.scale_factor());
        if self.devtools.is_some() { (full.height * 2 / 5).max(120).min(full.height) } else { 0 }
    }

    pub(crate) fn content_bounds(&self) -> Rect {
        let mut bounds = super::page_bounds(&self.window);
        let size = bounds.size.to_physical::<u32>(self.window.scale_factor());
        bounds.size = PhysicalSize::new(size.width, size.height.saturating_sub(self.dock_height())).into();
        bounds
    }

    pub(crate) fn devtools_bounds(&self) -> [i32; 4] {
        let full = super::page_bounds(&self.window);
        let size = full.size.to_physical::<u32>(self.window.scale_factor());
        let top = full.position.to_physical::<i32>(self.window.scale_factor()).y;
        [0, top + size.height.saturating_sub(self.dock_height()) as i32, size.width as i32, self.dock_height() as i32]
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
