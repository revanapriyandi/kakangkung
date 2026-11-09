use std::{collections::{HashMap, VecDeque}, rc::Rc, time::{Duration, Instant}};
use platform::{TabMemory, Thumbnail};
use tao::platform::windows::WindowExtWindows;
use super::Browser;
use crate::msg::Msg;

const MAX_PREVIEWS: usize = 8;
const REFRESH: Duration = Duration::from_secs(5);

struct Cached {
    tab: u64,
    revision: u64,
    image: Rc<Thumbnail>,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct Previews {
    pub(crate) window: Option<platform::TabPreview>,
    pub(crate) hovered: Option<(u64, f32, f32)>,
    images: VecDeque<Cached>,
    memory: HashMap<u64, (Instant, TabMemory)>,
    pending_image: Option<(u64, u64)>,
    failures: HashMap<u64, u8>,
    memory_due: Option<Instant>,
    pub(crate) capture_due: Option<(u64, Instant)>,
}

impl Previews {
    pub(crate) fn status(&self, id: u64) -> serde_json::Value {
        serde_json::json!({"visible":self.window.as_ref().is_some_and(|window|window.is_visible()),
            "thumbnail":self.images.iter().any(|entry|entry.tab==id),"memory":self.memory.get(&id).map(|(_,value)|value.private_ws),
            "capture_pending":self.pending_image.is_some_and(|(tab,_)|tab==id),
            "capture_failures":self.failures.get(&id).copied().unwrap_or(0),
            "cached":self.images.len(),"cache_bytes":self.images.iter().map(|entry|entry.image.pixels.len()).sum::<usize>()})
    }
    fn insert(&mut self, tab: u64, revision: u64, image: Thumbnail) {
        if image.width > 320 || image.height > 180 || image.width == 0 || image.height == 0
            || image.pixels.len() != image.width as usize * image.height as usize * 4 { return; }
        self.images.retain(|entry| entry.tab != tab);
        self.images.push_back(Cached { tab, revision, image: Rc::new(image), at: Instant::now() });
        while self.images.len() > MAX_PREVIEWS { self.images.pop_front(); }
    }

    pub(crate) fn forget(&mut self, tab: u64) {
        self.images.retain(|entry| entry.tab != tab);
        self.memory.remove(&tab);
        self.failures.remove(&tab);
        if self.capture_due.is_some_and(|(id,_)|id==tab) { self.capture_due=None; }
        if self.pending_image.is_some_and(|(id,_)|id==tab) { self.pending_image=None; }
        if self.hovered.is_some_and(|(id,_,_)| id == tab) { self.hide(); }
    }

    pub(crate) fn hide(&mut self) {
        self.hovered = None;
        self.memory_due = None;
        if let Some(window) = &self.window { window.hide(); }
    }
}

impl Browser {
    pub(crate) fn preview_document_ready(&mut self, id: u64, url: &str) {
        let Some(index) = self.index(id) else { return };
        let tab = &mut self.tabs[index];
        if tab.document_ready || tab.view.url.split('#').next() != url.split('#').next() { return; }
        tab.document_ready = true;
        self.previews.failures.remove(&id);
        if self.visible_with(self.active).contains(&id) {
            self.previews.capture_due = Some((id, Instant::now() + Duration::from_millis(700)));
        }
    }

    pub(crate) fn capture_tab_preview(&mut self, id: u64) {
        let Some(tab) = self.index(id).map(|i| &self.tabs[i]) else { return; };
        if !tab.document_ready || !self.visible_with(self.active).contains(&id) || self.previews.pending_image.is_some() { return; }
        let revision = tab.revision;
        if let Some(entry) = self.previews.images.iter().find(|entry| entry.tab == id && entry.revision == revision && entry.at.elapsed() < REFRESH) {
            self.previews.capture_due = Some((id, entry.at + REFRESH));
            return;
        }
        if self.previews.failures.get(&id).copied().unwrap_or(0) >= 3 { return; }
        let Some(wv) = tab.webview.as_ref() else { return; };
        let proxy = self.proxy.clone();
        match platform::capture_thumbnail(wv, move |result| {
            let _ = proxy.send_event(Msg::TabThumbnail { id, revision, result });
        }) {
            Ok(()) => self.previews.pending_image = Some((id, revision)),
            Err(error) => self.preview_capture_failed(id, error.to_string()),
        }
    }

    fn preview_capture_failed(&mut self, id: u64, error: String) {
        let attempts = self.previews.failures.entry(id).or_default();
        *attempts = attempts.saturating_add(1);
        let attempts = *attempts;
        eprintln!("tab preview {id}: capture attempt {attempts}: {error}");
        if attempts < 3 && self.visible_with(self.active).contains(&id) {
            self.previews.capture_due = Some((id, Instant::now() + Duration::from_millis(500 * attempts as u64)));
        }
    }

    fn request_preview_memory(&mut self, id: u64) {
        let Some(wv) = self.index(id).and_then(|i|self.tabs[i].webview.as_ref()) else { return; };
        let revision = self.tabs[self.index(id).unwrap_or(0)].revision;
        let proxy = self.proxy.clone();
        let _ = platform::tab_memory(wv, move |result| {
            let _ = proxy.send_event(Msg::TabMemory { id, revision, result });
        });
    }

    pub(crate) fn hover_tab(&mut self, id: u64, x: f32, bottom: f32) {
        if id == 0 || self.index(id).is_none() || self.overlay_open || self.ext_bubble.is_some() {
            self.previews.hide();
            return;
        }
        self.previews.hovered = Some((id, x, bottom));
        self.draw_tab_preview();
        self.capture_tab_preview(id);
        self.request_preview_memory(id);
        self.previews.memory_due = Some(Instant::now() + Duration::from_secs(2));
    }

    pub(crate) fn thumbnail_ready(&mut self, id: u64, revision: u64, result: Result<Thumbnail, String>) {
        if self.previews.pending_image == Some((id, revision)) { self.previews.pending_image = None; }
        if !self.index(id).is_some_and(|i| self.tabs[i].revision == revision) { return; }
        match result {
            Ok(image) => { self.previews.failures.remove(&id); self.previews.insert(id, revision, image); }
            Err(error) => self.preview_capture_failed(id, error),
        }
        if self.previews.hovered.is_some_and(|(hover,_,_)| hover == id) { self.draw_tab_preview(); }
    }

    pub(crate) fn preview_memory_ready(&mut self, id: u64, revision: u64, result: Result<TabMemory, String>) {
        if !self.index(id).is_some_and(|i| self.tabs[i].revision == revision) { return; }
        if let Ok(memory) = result { self.previews.memory.insert(id, (Instant::now(), memory)); }
        if self.previews.hovered.is_some_and(|(hover,_,_)| hover == id) { self.draw_tab_preview(); }
    }

    fn draw_tab_preview(&mut self) {
        let Some((id,x,bottom)) = self.previews.hovered else { return; };
        let Some(tab) = self.index(id).map(|i|&self.tabs[i]) else { return; };
        let thumbnail = self.previews.images.iter().find(|entry|entry.tab == id && entry.revision == tab.revision).map(|entry|entry.image.clone());
        let memory = match (tab.webview.is_some(), self.previews.memory.get(&id)) {
            (false, _) => "No page renderer allocated".into(),
            (true, Some((at, memory))) if at.elapsed() < Duration::from_secs(10) || tab.state==tabs::Lifecycle::Suspended => {
                if memory.heap_only { format!("JS heap{}: {:.1} MiB",if at.elapsed()>=Duration::from_secs(10) { " (cached)" } else { " only" },memory.private_ws as f64/1048576.0) }
                else { format!("Memory usage: {:.1} MiB{}", memory.private_ws as f64 / 1048576.0, if memory.shared { " (shared)" } else { "" }) }
            },
            _ => "Memory: not available yet".into(),
        };
        let status = if thumbnail.is_some() { "Last captured preview" }
            else if crate::native::page_of(&tab.view.url).is_some() { "Browser page" }
            else if tab.webview.is_none() { "Open this tab to capture a preview" }
        else if self.previews.pending_image.is_some_and(|(pending,_)|pending==id) { "Capturing preview…" }
        else if !tab.document_ready { "Page is loading…" }
        else if !self.visible_with(self.active).contains(&id) { "Open this tab to capture a preview" }
        else if self.previews.failures.get(&id).copied().unwrap_or(0) >= 3 { "Preview unavailable for this page" }
        else { "Preparing preview…" };
        let note = if self.previews.memory.get(&id).is_some_and(|(_,memory)|memory.heap_only) { "Excludes images, video and GPU memory" } else { "" };
        let site = if tab.view.url.starts_with("https://") || tab.view.url.starts_with("http://") {
            tab.view.url.split_once("://").map(|(_,rest)|rest.split(['/', '?', '#']).next().unwrap_or(rest)).unwrap_or_default().to_string()
        } else { ui::display_url(&tab.view.url) };
        let data = platform::TabPreviewData { title:tab.view.title.clone(),site,memory,note:note.into(),status:status.into(),thumbnail };
        if self.previews.window.is_none() { self.previews.window = platform::TabPreview::new(self.window.hwnd()).ok(); }
        if let Some(window) = &self.previews.window { let _ = window.show(data,x,bottom,self.dark_ui()); }
    }

    pub(crate) fn preview_tick(&mut self) -> Option<Instant> {
        let now = Instant::now();
        if self.previews.capture_due.is_some_and(|(_,due)| now >= due) {
            let (id,_) = self.previews.capture_due.take()?;
            self.capture_tab_preview(id);
            self.request_preview_memory(id);
        }
        if self.previews.memory_due.is_some_and(|due| now >= due) {
            if let Some((id,_,_)) = self.previews.hovered {
                self.request_preview_memory(id);
                self.previews.memory_due = Some(now + Duration::from_secs(2));
            } else { self.previews.memory_due = None; }
        }
        self.previews.capture_due.map(|(_,due)| due).into_iter().chain(self.previews.memory_due).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_is_bounded_and_replaces_a_tabs_old_document() {
        let mut cache = Previews::default();
        for id in 0..100 { cache.insert(id,0,Thumbnail {width:320,height:180,pixels:vec![0;320*180*4]}); }
        assert_eq!(cache.images.len(),8);
        assert_eq!(cache.images.iter().map(|entry|entry.image.pixels.len()).sum::<usize>(),1_843_200);
        cache.insert(99,1,Thumbnail {width:1,height:1,pixels:vec![0;4]});
        assert_eq!(cache.images.iter().filter(|entry|entry.tab==99).count(),1);
        cache.forget(99);
        assert!(!cache.images.iter().any(|entry|entry.tab==99));
        cache.insert(200,0,Thumbnail {width:321,height:180,pixels:vec![]});
        assert!(!cache.images.iter().any(|entry|entry.tab==200));
    }
}
