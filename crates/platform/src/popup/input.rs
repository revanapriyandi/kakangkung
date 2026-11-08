//! Mouse and keyboard of the pop-up: hover, click to pick (or close a row with its ×, or fold a
//! section), wheel to scroll, and the tab-search filter box (type to filter, ↑/↓, Enter, Escape;
//! closes when it loses focus).

use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, GetDC, InvalidateRect, ReleaseDC, SelectObject, SetBkColor, SetBkMode, SetTextColor, TextOutW, HDC, HGDIOBJ,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, FF_DONTCARE, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetWindowLongPtrW, IsWindowVisible, KillTimer, MoveWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, WM_TIMER, SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow, CREATESTRUCTW, GWLP_USERDATA, MA_NOACTIVATE, SW_SHOW,
    WM_CHAR, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT,
    WM_GETFONT, WM_SETFONT,
};

use super::{Inner, BUTTONS_ROW, BUTTON_GAP, CLOSE_W, FILTER_H, HEADER_ROW, MAIN_BUTTON_W, OTHER_BUTTON_W, PADDING, SEP_ROW, TWO_LINE_ROW};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use crate::frame::style::{colorref, palette};
use crate::frame::{edit_text, lparam_point, scale_of};

impl Inner {
    fn send(&mut self, cmd: String) {
        (self.on_command)(cmd);
    }

    pub(super) fn filter_hidden(&self) -> bool {
        // SAFETY: plain query on our own child control.
        unsafe { !IsWindowVisible(self.filter).as_bool() }
    }

    pub(super) fn is_header(&self, k: usize) -> bool {
        self.rows[k].kind == "header"
    }

    /// Rows the selection skips: section titles, a dialog's text and its buttons.
    pub(super) fn inert(&self, k: usize) -> bool {
        matches!(self.rows[k].kind.as_str(), "header" | "note" | "buttons" | "sep")
    }

    /// Main and other button of a dialog's buttons row spanning `row` (DIPs).
    pub(super) fn button_rects(row: D2D_RECT_F) -> (D2D_RECT_F, D2D_RECT_F) {
        let (top, h) = (row.top + (row.bottom - row.top - 34.0) / 2.0, 34.0);
        let main = D2D_RECT_F { left: row.right - 8.0 - MAIN_BUTTON_W, top, right: row.right - 8.0, bottom: top + h };
        let other = D2D_RECT_F { left: main.left - BUTTON_GAP - OTHER_BUTTON_W, top, right: main.left - BUTTON_GAP, bottom: top + h };
        (main, other)
    }

    /// The zoom row's buttons: −, the value (reset), +, full screen.
    pub(super) fn zoom_rects(row: D2D_RECT_F) -> [D2D_RECT_F; 4] {
        let (top, h) = (row.top + (row.bottom - row.top - 28.0) / 2.0, 28.0);
        let at = |right: f32, w: f32| D2D_RECT_F { left: right - w, top, right, bottom: top + h };
        let full = at(row.right - 8.0, 28.0);
        let plus = at(full.left - 14.0, 28.0);
        let value = at(plus.left, 52.0);
        let minus = at(value.left, 28.0);
        [minus, value, plus, full]
    }

    /// The command of the zoom row's button at `x`, if any.
    pub(super) fn zoom_at(&self, x: f32) -> Option<&'static str> {
        let row = D2D_RECT_F { left: PADDING, top: 0.0, right: self.width() - PADDING, bottom: 28.0 };
        let rects = Self::zoom_rects(row);
        rects.iter().position(|r| x >= r.left && x < r.right).map(|i| ["zoomout", "zoomreset", "zoomin", "fullscreen"][i])
    }

    /// Which button of a buttons row `x` is over: Some(true) = the other one, Some(false) = the main one.
    fn button_at(&self, index: usize, x: f32) -> Option<bool> {
        let &k = self.shown.get(index)?;
        if self.rows[k].kind != "buttons" {
            return None;
        }
        let row = D2D_RECT_F { left: PADDING, top: 0.0, right: self.width() - PADDING, bottom: BUTTONS_ROW };
        let (main, other) = Self::button_rects(row);
        if x >= main.left && x < main.right {
            Some(false)
        } else if x >= other.left && x < other.right {
            Some(true)
        } else {
            None
        }
    }

    /// Height of row `k` of `rows`.
    pub(super) fn height_of(&self, k: usize) -> f32 {
        match &self.rows[k] {
            r if r.kind == "header" => HEADER_ROW,
            r if r.kind == "buttons" => BUTTONS_ROW,
            r if r.kind == "sep" => SEP_ROW,
            r if !r.sub.is_empty() => TWO_LINE_ROW,
            _ => self.row_h,
        }
    }

    /// The first pickable entry of the list on show.
    pub(super) fn first_item(&self) -> Option<usize> {
        self.shown.iter().position(|&k| !self.inert(k))
    }

    /// The rows on show: those matching `needle`, without the folded sections' rows; a section
    /// title stays while its section has matches (so a folded one can be opened again).
    pub(super) fn rebuild(&mut self, needle: &str) {
        let needle = needle.to_lowercase();
        let matches = |k: usize| {
            let r = &self.rows[k];
            needle.is_empty() || format!("{} {} {}", r.title, r.detail, r.sub).to_lowercase().contains(&needle)
        };
        let mut shown = vec![];
        let mut header: Option<usize> = None;
        let mut found = false;
        for k in 0..self.rows.len() {
            if self.is_header(k) {
                header = Some(k);
                found = false;
                continue;
            }
            if !matches(k) {
                continue;
            }
            if let (Some(h), false) = (header, found) {
                shown.push(h);
                found = true;
            }
            if !header.is_some_and(|h| self.collapsed.contains(&self.rows[h].id)) {
                shown.push(k);
            }
        }
        self.shown = shown;
        self.scroll = self.scroll.min(self.shown.len().saturating_sub(1));
    }

    /// Row slot under a point (DIPs), as an index into `shown`.
    fn row_at(&self, y: f32) -> Option<usize> {
        let mut top = self.list_top();
        if y < top {
            return None;
        }
        for (slot, &k) in self.shown.iter().enumerate().skip(self.scroll) {
            let h = self.height_of(k);
            if y < top + h {
                return Some(slot);
            }
            top += h;
        }
        None
    }

    fn width(&self) -> f32 {
        let mut rc = RECT::default();
        // SAFETY: plain query on our own window.
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        (rc.right - rc.left) as f32 / scale_of(self.hwnd)
    }

    /// Is `x` over the × of row slot `index` (or a dialog's other button)?
    fn on_close(&self, index: usize, x: f32) -> bool {
        if let Some(other) = self.button_at(index, x) {
            return other;
        }
        self.shown.get(index).is_some_and(|&k| !self.rows[k].close.is_empty()) && x >= self.width() - PADDING - CLOSE_W
    }

    fn pick(&mut self, index: usize, close: bool) {
        let Some(&k) = self.shown.get(index) else { return };
        if matches!(self.rows[k].kind.as_str(), "note" | "sep") {
            return;
        }
        if self.is_header(k) {
            // A section title with an id folds or unfolds its section.
            let id = self.rows[k].id.clone();
            if !id.is_empty() {
                match self.collapsed.iter().position(|c| *c == id) {
                    Some(at) => {
                        self.collapsed.remove(at);
                    }
                    None => self.collapsed.push(id),
                }
                self.apply_filter();
                self.repaint();
            }
            return;
        }
        let id = if close { self.rows[k].close.clone() } else { self.rows[k].id.clone() };
        let cmd = format!("pick\t{}\t{id}", self.kind);
        self.send(cmd);
    }

    fn apply_filter(&mut self) {
        let needle = if self.filter_hidden() { String::new() } else { edit_text(self.filter) };
        self.rebuild(&needle);
        self.selected = self.first_item();
    }

    /// Move the selection by one entry, over section titles, round the list.
    fn step(&mut self, down: bool) {
        let n = self.shown.len();
        let mut s = self.selected.unwrap_or(if down { n - 1 } else { 0 });
        for _ in 0..n {
            s = if down { (s + 1) % n } else { (s + n - 1) % n };
            if !self.inert(self.shown[s]) {
                break;
            }
        }
        self.selected = Some(s);
        // Keep the selection in view.
        if s < self.scroll {
            self.scroll = s;
        }
        let mut rc = RECT::default();
        // SAFETY: plain query on our own window.
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let room = (rc.bottom - rc.top) as f32 / scale_of(self.hwnd) - self.list_top() - PADDING;
        while self.scroll < s && self.shown[self.scroll..=s].iter().map(|&k| self.height_of(k)).sum::<f32>() > room {
            self.scroll += 1;
        }
        self.repaint();
    }

    fn repaint(&self) {
        // SAFETY: repaint request for our own window.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

/// Empty, show and place the filter box for a new list.
pub(super) fn reset_filter(i: &mut Inner, scale: f32, width: f32) {
    // SAFETY: plain calls on the live filter box (a child of the pop-up).
    unsafe {
        let font = CreateFontW(-(14.0 * scale) as i32, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"));
        SendMessageW(i.filter, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        let _ = SetWindowTextW(i.filter, w!(""));
        let (x, y, h) = (PADDING + 44.0, PADDING + (FILTER_H - 8.0 - 22.0) / 2.0, 22.0);
        let _ = MoveWindow(i.filter, (x * scale) as i32, (y * scale) as i32, ((width - x - 2.0 * PADDING - 110.0) * scale) as i32, (h * scale) as i32, true);
        let _ = ShowWindow(i.filter, SW_SHOW);
    }
    i.selected = i.first_item();
    i.repaint();
}

pub(super) unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY (whole function): our own window on the UI thread; GWLP_USERDATA holds the boxed
    // RefCell<Inner> from WM_NCCREATE until WM_DESTROY.
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            if let Some(cell) = (cs.lpCreateParams as *const RefCell<Inner>).as_ref() {
                cell.borrow_mut().hwnd = hwnd;
            }
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<Inner>;
        let Some(cell) = ptr.as_ref() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        if msg == WM_DESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let inner = Box::from_raw(ptr).into_inner();
            let _ = DeleteObject(inner.filter_brush.1.into());
            return LRESULT(0);
        }
        let Ok(mut i) = cell.try_borrow_mut() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        let scale = scale_of(hwnd);
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_TIMER if wp.0 == super::TIMER_REVEAL => {
                let Some((start, full)) = i.reveal else {
                    let _ = KillTimer(Some(hwnd), super::TIMER_REVEAL);
                    return LRESULT(0);
                };
                let t = crate::motion::eased(start, 0.0, super::REVEAL_MS);
                let h = (full as f32 * (super::REVEAL_FROM + (1.0 - super::REVEAL_FROM) * t)) as i32;
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let _ = SetWindowPos(hwnd, None, 0, 0, rc.right - rc.left, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                if t >= 1.0 {
                    i.reveal = None;
                    let _ = KillTimer(Some(hwnd), super::TIMER_REVEAL);
                }
                i.repaint();
                return LRESULT(0);
            }
            // Clicking the list must not take the keyboard from the address bar (a menu has it).
            WM_MOUSEACTIVATE if !i.menu_focus => return LRESULT(MA_NOACTIVATE as isize),
            // A menu: arrows move, Enter picks, Escape closes; clicking elsewhere closes it.
            WM_KEYDOWN if i.menu_focus => {
                let key = wp.0 as u16;
                let zoom_row = i.selected.and_then(|s| i.shown.get(s)).is_some_and(|&k| i.rows[k].kind == "zoom");
                if zoom_row && (key == VK_LEFT.0 || key == VK_RIGHT.0) {
                    // On the zoom row ← and → zoom out and in.
                    let cmd = if key == VK_LEFT.0 { "zoomout" } else { "zoomin" };
                    let kind = i.kind.clone();
                    i.send(format!("pick\t{kind}\t{cmd}"));
                } else if key == VK_RETURN.0 {
                    if let Some(index) = i.selected {
                        i.pick(index, false);
                    }
                } else if key == VK_RIGHT.0 {
                    // → opens the selected row's submenu beside the menu.
                    if let Some(index) = i.selected.filter(|&s| i.shown.get(s).is_some_and(|&k| i.rows[k].id.starts_with("submenu:"))) {
                        i.pick(index, false);
                    }
                } else if i.kind == "submenu" && (key == VK_LEFT.0 || key == VK_ESCAPE.0) {
                    // ← or Escape in a submenu: back to the menu it came from.
                    i.send("menuback".into());
                } else if key == VK_ESCAPE.0 {
                    i.send("overlayclose".into());
                } else if (key == VK_DOWN.0 || key == VK_UP.0) && i.first_item().is_some() {
                    i.step(key == VK_DOWN.0);
                }
                return LRESULT(0);
            }
            WM_KILLFOCUS if i.menu_focus => {
                // Opening a menu from the focused address bar makes Windows hand the keyboard
                // back to the browser window a moment later: take it again instead of closing.
                // ponytail: a time window, not the cause; a click on the browser within it keeps the menu open.
                let just_opened = i.menu_opened.is_some_and(|at| at.elapsed().as_millis() < super::REFOCUS_MS);
                let to = HWND(wp.0 as _);
                // The keyboard went to the menu's submenu, or back from it: both stay open.
                let sibling = !to.is_invalid() && to != hwnd && windows::Win32::UI::WindowsAndMessaging::GetWindow(to, windows::Win32::UI::WindowsAndMessaging::GW_OWNER).ok() == Some(i.owner);
                if sibling {
                } else if just_opened && to == i.owner {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(hwnd), super::WM_REFOCUS, WPARAM(0), LPARAM(0));
                } else {
                    i.send("overlayhide".into());
                }
            }
            super::WM_REFOCUS => {
                if i.menu_focus && IsWindowVisible(hwnd).as_bool() {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
                }
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = i.paint();
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let (x, y) = lparam_point(lp, scale);
                let hover = i.row_at(y);
                let on_close = hover.is_some_and(|h| i.on_close(h, x));
                let zoom_part = |i: &Inner| i.zoom_at(i.mouse_x);
                let before = zoom_part(&i);
                i.mouse_x = x;
                if before != zoom_part(&i) {
                    i.repaint();
                }
                if hover != i.hover && i.kind == "menu" {
                    // Pointing at a row with a submenu opens it beside the menu (any other row
                    // closes it), like Chrome's menus.
                    let id = hover.and_then(|h| i.shown.get(h)).map(|&k| i.rows[k].id.clone()).unwrap_or_default();
                    i.send(format!("hover\tmenu\t{id}"));
                }
                if hover != i.hover || on_close != i.hover_close {
                    i.hover = hover;
                    i.hover_close = on_close;
                    i.repaint();
                }
            }
            WM_MOUSELEAVE => {
                i.hover = None;
                i.hover_close = false;
                i.repaint();
            }
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_point(lp, scale);
                if i.row_at(y).and_then(|s| i.shown.get(s)).is_some_and(|&k| i.rows[k].kind == "zoom") {
                    // The zoom row: only its buttons do something.
                    if let Some(cmd) = i.zoom_at(x) {
                        let kind = i.kind.clone();
                        i.send(format!("pick\t{kind}\t{cmd}"));
                    }
                } else if let Some(index) = i.row_at(y) {
                    // A dialog's buttons row picks only on a button.
                    let on_button = i.button_at(index, x);
                    let buttons = i.shown.get(index).is_some_and(|&k| i.rows[k].kind == "buttons");
                    if !buttons || on_button.is_some() {
                        let close = i.on_close(index, x);
                        i.pick(index, close);
                    }
                }
                return LRESULT(0);
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xffff) as i16;
                let max = i.shown.len().saturating_sub(1);
                i.scroll = if delta < 0 { (i.scroll + 3).min(max) } else { i.scroll.saturating_sub(3) };
                i.repaint();
                return LRESULT(0);
            }
            // The filter box in the list's colours (the pill behind it).
            WM_CTLCOLOREDIT => {
                let p = palette(i.dark, false);
                let pill = if i.dark { 0x2a2930 } else { 0xffffff };
                let hdc = HDC(wp.0 as _);
                SetTextColor(hdc, colorref(p.text));
                SetBkColor(hdc, colorref(pill));
                if i.filter_brush.0 != pill {
                    let _ = DeleteObject(i.filter_brush.1.into());
                    i.filter_brush = (pill, CreateSolidBrush(colorref(pill)));
                }
                return LRESULT(i.filter_brush.1.0 as isize);
            }
            // The filter box changed: list only the matching rows.
            WM_COMMAND if (wp.0 >> 16) as u32 == 0x0300 => {
                i.apply_filter();
                i.repaint();
            }
            _ => {}
        }
        drop(i);
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

/// Keys of the tab-search filter box.
pub(super) unsafe extern "system" fn filter_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    // SAFETY: `data` is the pop-up's boxed RefCell<Inner>, alive while the pop-up (our parent) is.
    unsafe {
        let Some(cell) = (data as *const RefCell<Inner>).as_ref() else { return DefSubclassProc(hwnd, msg, wp, lp) };
        match msg {
            WM_KEYDOWN => {
                let Ok(mut i) = cell.try_borrow_mut() else { return DefSubclassProc(hwnd, msg, wp, lp) };
                let key = wp.0 as u16;
                if key == VK_RETURN.0 {
                    if i.input_mode {
                        let cmd = format!("input\t{}\t{}", i.kind, edit_text(hwnd));
                        i.send(cmd);
                    } else if let Some(index) = i.selected {
                        i.pick(index, false);
                    }
                } else if key == VK_ESCAPE.0 {
                    i.send("overlayclose".into());
                } else if (key == VK_DOWN.0 || key == VK_UP.0) && i.first_item().is_some() {
                    i.step(key == VK_DOWN.0);
                } else {
                    drop(i);
                    return DefSubclassProc(hwnd, msg, wp, lp);
                }
                return LRESULT(0);
            }
            // No beep for Enter / Escape.
            WM_CHAR if wp.0 == 13 || wp.0 == 27 => return LRESULT(0),
            // "Search Tabs" while the box is empty (a plain EDIT has no placeholder of its own).
            WM_PAINT => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                if edit_text(hwnd).is_empty() {
                    let dark = cell.try_borrow().map(|i| i.dark).unwrap_or(false);
                    let hdc = GetDC(Some(hwnd));
                    let font = SendMessageW(hwnd, WM_GETFONT, None, None);
                    let old = SelectObject(hdc, HGDIOBJ(font.0 as _));
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, colorref(palette(dark, false).muted));
                    let placeholder = cell.try_borrow().map(|i| i.placeholder.clone()).unwrap_or_default();
                    let text: Vec<u16> = placeholder.encode_utf16().collect();
                    let _ = TextOutW(hdc, 2, 0, &text);
                    SelectObject(hdc, old);
                    ReleaseDC(Some(hwnd), hdc);
                }
                return r;
            }
            // Clicking anywhere else closes tab search, like a menu.
            WM_KILLFOCUS => {
                if let Ok(mut i) = cell.try_borrow_mut() {
                    i.send("overlayhide".into());
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
