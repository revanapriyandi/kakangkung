//! The address bar's behaviour: typing asks for suggestions, the first match is completed
//! inline, ↑/↓ walk the suggestion rows, Enter goes, Escape steps back. The text box itself is
//! the native control in the frame (`platform::frame`); the rows are shown by the pop-up.

use serde_json::{json, Value as Json};

use crate::browser::Browser;
use crate::msg::Msg;

/// Height of one suggestion row and the list's padding in the pop-up (platform::popup).
const ROW_H: f64 = 36.0;
const LIST_PADDING: f64 = 10.0;

/// What the person is doing in the address bar.
#[derive(Debug, Default)]
pub(crate) struct Omnibox {
    /// Request number of the latest typing; older answers are ignored.
    pub(crate) seq: u64,
    /// The text as typed (without the inline completion).
    pub(crate) typed: String,
    /// The last keystroke removed text: no inline completion then.
    deleting: bool,
    /// Suggestion rows on show and the selected one (0 = what was typed).
    pub(crate) rows: Vec<Json>,
    pub(crate) selected: usize,
    /// The typing is in the New Tab page's search box, not the address bar.
    pub(crate) in_page: bool,
}

impl Browser {
    /// Text of the box being typed in (the address bar, or the New Tab page's search box).
    fn box_text(&self) -> String {
        if self.omnibox.in_page { self.page.fakebox_text() } else { self.frame.omnibox_text() }
    }

    fn set_box_text(&self, text: &str, select_from: usize) {
        if self.omnibox.in_page {
            self.page.set_fakebox_text(text, select_from);
        } else {
            self.frame.set_omnibox_text(text, select_from);
        }
    }

    fn box_has_focus(&self) -> bool {
        if self.omnibox.in_page { self.page.fakebox_has_focus() } else { self.frame.omnibox_has_focus() }
    }

    /// The text in the box changed by typing.
    pub(crate) fn on_omnibox_change(&mut self, text: &str) {
        let o = &mut self.omnibox;
        o.deleting = text.chars().count() < o.typed.chars().count();
        o.typed = text.to_string();
        o.seq += 1;
        if text.trim().is_empty() {
            self.close_suggestions();
            return;
        }
        let seq = self.omnibox.seq;
        self.request_suggestions(seq, text.to_string());
    }

    /// Enter, Escape, ↑ and ↓ in the box.
    pub(crate) fn on_omnibox_key(&mut self, key: &str) -> bool {
        match key {
            "up" | "down" if !self.omnibox.rows.is_empty() => {
                let n = self.omnibox.rows.len();
                let o = &mut self.omnibox;
                o.selected = if key == "down" { (o.selected + 1) % n } else { (o.selected + n - 1) % n };
                let row = &o.rows[o.selected];
                let text = if row["kind"] == "search" { o.typed.clone() } else { row["display"].as_str().unwrap_or("").to_string() };
                let len = text.chars().count();
                self.set_box_text(&text, len);
                self.popup.highlight(Some(self.omnibox.selected));
            }
            "enter" => {
                let picked = (self.omnibox.selected > 0).then(|| self.omnibox.rows[self.omnibox.selected]["url"].as_str().map(String::from)).flatten();
                let text = picked.unwrap_or_else(|| self.box_text());
                self.close_suggestions();
                self.omnibox.typed.clear();
                self.omnibox.in_page = false;
                if !text.trim().is_empty() {
                    self.navigate(&text);
                }
            }
            "escape" if self.omnibox.in_page => self.close_suggestions(),
            "escape" => {
                if self.omnibox.rows.is_empty() {
                    self.show_address(false);
                    let _ = self.on_ipc("focuspage");
                } else {
                    let typed = self.omnibox.typed.clone();
                    let len = typed.chars().count();
                    self.close_suggestions();
                    self.frame.set_omnibox_text(&typed, len);
                }
            }
            _ => {}
        }
        !self.tabs.is_empty()
    }

    /// Editing starts: the full address, all selected (the frame selects it). The focus report
    /// arrives through the event queue, so typing may already have begun: then it is left alone.
    pub(crate) fn on_omnibox_focus(&mut self) {
        if !self.omnibox.typed.is_empty() {
            return;
        }
        self.show_address(true);
    }

    /// Editing ends: back to the short address.
    pub(crate) fn on_omnibox_blur(&mut self) {
        self.close_suggestions();
        self.omnibox.typed.clear();
        self.show_address(false);
    }

    /// Put the active tab's address in the box: full (`editing`) or short.
    fn show_address(&self, editing: bool) {
        let Some(t) = self.tabs.iter().find(|t| t.view.id == self.active) else { return };
        let text = if editing {
            if t.view.url.starts_with(ui::INTERNAL) { ui::display_url(&t.view.url) } else { t.view.url.clone() }
        } else if self.settings.full_urls && !t.view.url.starts_with(ui::INTERNAL) {
            t.view.url.clone()
        } else {
            ui::display_url(&t.view.url)
        };
        self.frame.set_omnibox_text(&text, if editing { 0 } else { text.chars().count() });
    }

    /// Ask history/bookmarks (storage thread) and the search engine (own thread) for request
    /// `seq`; the answers arrive as `Msg::Suggest` and `Msg::RemoteSuggest`.
    pub(crate) fn request_suggestions(&mut self, seq: u64, text: String) {
        self.suggest_state = (seq, None, vec![]);
        let remote = if self.settings.suggestions { ui::suggest_url(&self.engine_setting, &text) } else { None };
        if let Some(url) = remote {
            let p = self.proxy.clone();
            std::thread::spawn(move || {
                let items = platform::http::get(&url, super::ipc::SUGGEST_TIMEOUT_MS)
                    .map(|body| ui::parse_suggestions(&body, super::ipc::MAX_ENGINE_SUGGESTIONS))
                    .unwrap_or_default();
                let _ = p.send_event(Msg::RemoteSuggest { seq, items });
            });
        }
        let (p, engine, history) = (self.proxy.clone(), self.search.clone(), self.settings.history_suggestions);
        self.job(Box::new(move |s| {
            let json = super::menus::suggest_rows(s.filter(|_| history), &text, &engine).to_string();
            let _ = p.send_event(Msg::Suggest { seq, json });
        }));
    }

    /// Rows for request `seq` are ready: complete inline and show them under the box.
    pub(crate) fn show_suggestions(&mut self, seq: u64, answer: Json) {
        if seq != self.omnibox.seq || !self.box_has_focus() {
            return;
        }
        let rows = answer["rows"].as_array().cloned().unwrap_or_default();
        let typed = self.omnibox.typed.clone();
        // Inline completion: fill in the rest of the first match and leave it selected.
        if let Some(inline) = answer["inline"].as_str().filter(|i| !i.is_empty()) {
            let fits = inline.to_lowercase().starts_with(&typed.to_lowercase());
            if fits && !self.omnibox.deleting && self.box_text() == typed {
                let rest: String = inline.chars().skip(typed.chars().count()).collect();
                self.set_box_text(&format!("{typed}{rest}"), typed.chars().count());
            }
        }
        self.omnibox.rows = rows;
        self.omnibox.selected = 0;
        if self.omnibox.rows.is_empty() {
            self.close_suggestions();
            return;
        }
        // Under the box being typed in; the page window sits right under the frame.
        let (x, y, w, h) = match self.page.fakebox_rect().filter(|_| self.omnibox.in_page) {
            Some((x, y, w, h)) => (x, y + platform::frame::FRAME_H, w, h),
            None => self.frame.omnibox_rect(),
        };
        let height = self.omnibox.rows.len() as f64 * ROW_H + LIST_PADDING;
        let data = json!({ "rows": self.omnibox.rows, "selected": 0 }).to_string();
        self.show_overlay("suggest", [x as f64, (y + h) as f64 + 2.0, w as f64, height], &data);
    }

    pub(crate) fn close_suggestions(&mut self) {
        self.omnibox.rows.clear();
        self.omnibox.selected = 0;
        if self.overlay_open && self.overlay_kind == "suggest" {
            self.hide_overlay();
        }
    }
}
