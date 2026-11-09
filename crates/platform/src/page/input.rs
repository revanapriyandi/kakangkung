//! Mouse and keyboard of a page: smooth wheel scrolling, the slim scrollbar, hover, clicks on
//! controls (dropdowns open a native menu), the search box and the rows' text fields.

use std::cell::RefCell;
use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, SetBkColor, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, FF_DONTCARE, HDC,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN,
    VK_SHIFT, VK_UP,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetWindowLongPtrW, KillTimer, MoveWindow, SendMessageW, SetTimer, SetWindowLongPtrW, ShowWindow, CREATESTRUCTW, GWLP_USERDATA, SW_HIDE, SW_SHOW,
    WM_CHAR, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT,
    WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER,
};

use super::layout::{layout, HEADER_H};
use super::model::Control;
use super::paint::{colors, Meter};
use super::model::Bounce;
use super::{Inner, BOUNCE_MS, HOP_MS, ENTER_MS, HOVER_MS, PRESS_MS, TILE_DELAY_MS, TILE_MS, TILE_STAGGER_MS, TIMER_ANIM, TIMER_SCROLL, TIMER_TOAST, TOAST_IN_MS, TOAST_OUT_MS, TOGGLE_MS};
use crate::motion;
use crate::frame::layout::inside;
use crate::frame::{edit_text, lparam_point, scale_of};
use crate::menu::{popup, MenuItem};

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Nav(usize),
    /// A whole row that acts on click (switch, choice, link); index into the layout's rows.
    Row(usize),
    /// Piece `n` of a row's control (a button, a dropdown, a field's button).
    Part(usize, usize),
    Search,
    /// The start page's search box (hands the typing to the address bar).
    Fakebox,
    Bar,
}

const WHEEL_STEP: f32 = 100.0;

impl Inner {
    fn send(&mut self, cmd: String) {
        (self.on_command)(cmd);
    }

    fn size(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        // SAFETY: plain query on our own window.
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let s = scale_of(self.hwnd);
        ((rc.right - rc.left) as f32 / s, (rc.bottom - rc.top) as f32 / s)
    }

    pub(super) fn max_scroll(&self) -> f32 {
        (self.layout.height - (self.size().1 - HEADER_H)).max(0.0)
    }

    /// Top and length of the scrollbar's track.
    pub(super) fn bar_track(&self, h: f32) -> (f32, f32) {
        (HEADER_H + 4.0, (h - HEADER_H - 8.0).max(1.0))
    }

    /// Lay the page out again for the window's size and the search text.
    pub(super) fn relayout(&mut self) {
        let (w, h) = self.size();
        let m = Meter { dw: &self.dw, fonts: &self.fonts };
        let filter = if self.doc.remote_search { "" } else { self.filter.as_str() };
        self.layout = layout(&self.doc, w, h, filter, &self.section, &m);
        let max = self.max_scroll();
        self.scroll = self.scroll.min(max);
        self.target = self.target.min(max);
        self.place_edits();
        self.repaint();
    }

    /// Switch to section `id` as its own page, from the top.
    pub(super) fn open_section(&mut self, id: &str) {
        self.section = id.to_string();
        self.scroll = 0.0;
        self.target = 0.0;
        self.start_enter();
        self.relayout();
    }

    /// Let the page come in: its clock starts at the first frame drawn, so a busy moment before
    /// that does not eat the start of the animation.
    pub(super) fn start_enter(&mut self) {
        self.enter = Instant::now();
        self.enter_pending = true;
        self.animate();
    }

    /// Keep drawing frames while something moves (see `animating`).
    pub(super) fn animate(&self) {
        // SAFETY: frame timer on our own window.
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_ANIM, motion::FRAME_MS, None);
        }
    }

    /// Is anything still moving: the page coming in (with its tiles), a hover, a switch?
    pub(super) fn animating(&self) -> bool {
        let tiles = TILE_DELAY_MS + 8.0 * TILE_STAGGER_MS + TILE_MS;
        let bounce = match self.doc.logo_bounce {
            Bounce::None => false,
            Bounce::Once => motion::progress(self.enter, 0.0, BOUNCE_MS) < 1.0,
            Bounce::Loop => motion::phase(self.enter, HOP_MS).is_some(),
        };
        bounce
            || motion::progress(self.enter, 0.0, ENTER_MS.max(tiles)) < 1.0
            || motion::progress(self.hover_at, 0.0, HOVER_MS) < 1.0
            || self.toggles.iter().any(|(_, at)| motion::progress(*at, 0.0, TOGGLE_MS) < 1.0)
            || self.press.is_some_and(|(_, at)| motion::progress(at, 0.0, PRESS_MS) < 1.0)
            || (!self.toast.is_empty() && motion::progress(self.toast_at, 0.0, TOAST_IN_MS) < 1.0)
            || self.toast_out.is_some()
    }

    pub(super) fn scroll_smooth(&mut self, to: f32) {
        if !motion::smooth_scroll() {
            self.scroll_now(to);
            return;
        }
        self.target = to.clamp(0.0, self.max_scroll());
        // SAFETY: animation timer on our own window.
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_SCROLL, 15, None);
        }
    }

    fn scroll_now(&mut self, to: f32) {
        self.scroll = to.clamp(0.0, self.max_scroll());
        self.target = self.scroll;
        self.place_edits();
        self.repaint();
    }

    /// Move the native fields with the content. A field shows only while it is fully in view and
    /// holds text or the keyboard (otherwise its placeholder is drawn in its place).
    fn place_edits(&mut self) {
        let scale = scale_of(self.hwnd);
        let (_, h) = self.size();
        // SAFETY: plain calls on our own child controls.
        unsafe {
            let focus = windows::Win32::UI::Input::KeyboardAndMouse::GetFocus();
            let font = self.font(scale);
            let place = |edit: HWND, r: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F, x_pad: f32, show: bool| {
                SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(0)));
                let (x, y) = (r.left + x_pad, (r.top + r.bottom) / 2.0 - 10.0);
                let _ = MoveWindow(edit, (x * scale) as i32, (y * scale) as i32, ((r.right - r.left - x_pad - 12.0) * scale) as i32, (20.0 * scale) as i32, true);
                let _ = ShowWindow(edit, if show { SW_SHOW } else { SW_HIDE });
            };
            if !self.doc.search.is_empty() {
                let r = self.layout.search_box();
                let show = focus == self.search || !self.filter.is_empty();
                place(self.search, r, 46.0, show);
            }
            let mut placed = vec![];
            for b in &self.layout.rows {
                let row = &self.doc.sections[b.section].cards[b.card][b.row];
                let Control::Text { id, .. } = &row.control else { continue };
                let Some(&(_, edit)) = self.edits.iter().find(|(e, _)| e == id) else { continue };
                let mut r = b.control[0];
                r.top += HEADER_H - self.scroll;
                r.bottom += HEADER_H - self.scroll;
                let visible = r.top >= HEADER_H && r.bottom <= h;
                place(edit, r, 12.0, visible && (focus == edit || !edit_text(edit).is_empty()));
                placed.push(edit);
            }
            if let (Some(edit), Some(mut r)) = (self.fakebox_edit(), self.layout.fakebox) {
                r.top += HEADER_H - self.scroll;
                r.bottom += HEADER_H - self.scroll;
                place(edit, r, 52.0, focus == edit || !edit_text(edit).is_empty());
                placed.push(edit);
            }
            // Fields of sections the search hides.
            for (_, edit) in self.edits.iter().filter(|(_, e)| !placed.contains(e)) {
                let _ = ShowWindow(*edit, SW_HIDE);
            }
        }
    }

    /// Font of the native fields, made once per DPI.
    fn font(&mut self, scale: f32) -> windows::Win32::Graphics::Gdi::HFONT {
        if self.edit_font.0 != scale {
            // SAFETY: GDI font creation; the old one is no longer used by any control after this.
            unsafe {
                if !self.edit_font.1.is_invalid() {
                    let _ = DeleteObject(self.edit_font.1.into());
                }
                let f = CreateFontW(-(14.0 * scale) as i32, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"));
                self.edit_font = (scale, f);
            }
        }
        self.edit_font.1
    }

    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        let (w, _) = self.size();
        if self.max_scroll() > 0.0 && x >= w - 14.0 && y >= HEADER_H {
            return Some(Target::Bar);
        }
        if y < HEADER_H {
            return (!self.doc.search.is_empty() && inside(&self.layout.search_box(), x, y)).then_some(Target::Search);
        }
        if self.layout.nav && x < super::layout::NAV_W {
            return (0..self.doc.nav.len()).find(|&k| inside(&self.layout.nav_item(k), x, y)).map(Target::Nav);
        }
        let cy = y - HEADER_H + self.scroll;
        if self.layout.fakebox.is_some_and(|f| inside(&f, x, cy)) {
            return Some(Target::Fakebox);
        }
        if let Some(k) = self.layout.chips.iter().position(|r| inside(r, x, cy)) {
            return Some(Target::Nav(k));
        }
        let (k, b) = self.layout.rows.iter().enumerate().find(|(_, b)| inside(&b.rect, x, cy))?;
        if let Some(n) = b.control.iter().position(|r| inside(r, x, cy)) {
            let row = &self.doc.sections[b.section].cards[b.card][b.row];
            if matches!(row.control, Control::Buttons(_) | Control::Select { .. } | Control::Text { .. } | Control::Entry { .. }) {
                return Some(Target::Part(k, n));
            }
        }
        let row = &self.doc.sections[b.section].cards[b.card][b.row];
        matches!(row.control, Control::Toggle { .. } | Control::Radio { .. } | Control::Link { .. } | Control::Entry { .. }).then_some(Target::Row(k))
    }

    fn click(&mut self, target: Target) {
        match target {
            Target::Nav(k) => {
                let id = self.doc.nav[k].id.clone();
                // A section opens as its own page (the search ends); the browser updates the
                // address. Other entries are links the browser opens.
                if self.doc.paged && self.doc.sections.iter().any(|s| s.id == id) {
                    if !self.filter.is_empty() {
                        let search = self.search;
                        // SAFETY: our own child control; its EN_CHANGE finds Inner borrowed and
                        // is ignored, so the filter is cleared here.
                        unsafe {
                            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(search, w!(""));
                        }
                        self.filter.clear();
                    }
                    self.open_section(&id);
                }
                self.send(format!("page\tnav\t{id}\t"));
            }
            Target::Fakebox => {
                if let Some(edit) = self.fakebox_edit() {
                    // SAFETY: our own child control.
                    unsafe {
                        let _ = SetFocus(Some(edit));
                    }
                    self.place_edits();
                    self.repaint();
                }
            }
            Target::Search => {
                // SAFETY: our own child control.
                unsafe {
                    let _ = ShowWindow(self.search, SW_SHOW);
                    let _ = SetFocus(Some(self.search));
                }
                self.repaint();
            }
            Target::Row(k) => {
                let b = &self.layout.rows[k];
                let (s, c, r) = (b.section, b.card, b.row);
                let cmd = match &mut self.doc.sections[s].cards[c][r].control {
                    Control::Toggle { id, on } => {
                        // Flip at once (the knob slides over); the browser's next description
                        // confirms it.
                        *on = !*on;
                        let cmd = format!("page\ttoggle\t{id}\t{}", if *on { "1" } else { "0" });
                        let id = id.clone();
                        // A switch flipped back while still sliding turns from where its knob is.
                        let start = self.toggles.iter().find(|(t, _)| *t == id).map_or_else(Instant::now, |(_, at)| motion::reverse(*at, TOGGLE_MS));
                        self.toggles.retain(|(t, at)| *t != id && motion::progress(*at, 0.0, TOGGLE_MS) < 1.0);
                        self.toggles.push((id, start));
                        self.animate();
                        cmd
                    }
                    Control::Radio { id, value, .. } => format!("page\tradio\t{id}\t{value}"),
                    Control::Link { id } | Control::Entry { id, .. } => format!("page\tlink\t{id}\t"),
                    _ => return,
                };
                self.repaint();
                self.send(cmd);
            }
            Target::Part(k, n) => {
                let b = self.layout.rows[k].clone();
                let row = self.doc.sections[b.section].cards[b.card][b.row].clone();
                match row.control {
                    Control::Buttons(buttons) => self.send(format!("page\tbutton\t{}\t", buttons[n].id)),
                    Control::Entry { remove, .. } => self.send(format!("page\tbutton\t{remove}\t")),
                    Control::Text { id, .. } if n == 0 => {
                        // The field: show its native box and type in it.
                        if let Some(&(_, edit)) = self.edits.iter().find(|(e, _)| *e == id) {
                            // SAFETY: our own child control.
                            unsafe {
                                let _ = ShowWindow(edit, SW_SHOW);
                                let _ = SetFocus(Some(edit));
                            }
                        }
                    }
                    Control::Text { id, .. } => {
                        let text = self.edits.iter().find(|(e, _)| *e == id).map(|(_, h)| edit_text(*h)).unwrap_or_default();
                        self.send(format!("page\ttext\t{id}\t{text}"));
                    }
                    Control::Select { id, value, options } => {
                        let items: Vec<MenuItem> = options
                            .iter()
                            .enumerate()
                            .map(|(k, (v, l))| MenuItem::Item { id: k as u32 + 1, label: l.clone(), shortcut: String::new(), enabled: true, checked: *v == value })
                            .collect();
                        let r = b.control[0];
                        let scale = scale_of(self.hwnd);
                        let mut pt = POINT { x: (r.left * scale) as i32, y: ((r.bottom + HEADER_H - self.scroll + 4.0) * scale) as i32 };
                        // SAFETY: plain conversion for our own window.
                        unsafe {
                            let _ = ClientToScreen(self.hwnd, &mut pt);
                        }
                        let owner = self.root();
                        if let Some(choice) = popup(owner, pt.x, pt.y, false, &items) {
                            let v = &options[choice as usize - 1].0;
                            self.send(format!("page\tselect\t{id}\t{v}"));
                        }
                    }
                    _ => {}
                }
            }
            Target::Bar => {}
        }
    }

    /// The browser window (owner of the dropdown menus).
    fn root(&self) -> isize {
        // SAFETY: plain query on our own window.
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetAncestor(self.hwnd, windows::Win32::UI::WindowsAndMessaging::GA_ROOT).0 as isize }
    }

    fn key(&mut self, vk: u16) -> bool {
        let page = self.size().1 - HEADER_H - 40.0;
        let to = match vk {
            k if k == VK_DOWN.0 => self.target + 40.0,
            k if k == VK_UP.0 => self.target - 40.0,
            k if k == VK_NEXT.0 => self.target + page,
            k if k == VK_PRIOR.0 => self.target - page,
            k if k == VK_HOME.0 => 0.0,
            k if k == VK_END.0 => self.max_scroll(),
            _ => return false,
        };
        self.scroll_smooth(to);
        true
    }
}

/// Browser shortcuts (Ctrl/Alt combinations and F-keys) as the frame reports them.
fn shortcut(vk: u16) -> Option<String> {
    // SAFETY: plain keyboard-state queries.
    let down = |k: u16| unsafe { GetKeyState(k as i32) } < 0;
    let (ctrl, shift, alt) = (down(VK_CONTROL.0), down(VK_SHIFT.0), down(VK_MENU.0));
    (ctrl || alt || (shift && vk == VK_ESCAPE.0) || (0x70..=0x7B).contains(&vk)).then(|| format!("key\t{vk}\t{}\t{}\t{}", ctrl as u8, shift as u8, alt as u8))
}

pub(super) unsafe extern "system" fn page_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
            let _ = DeleteObject(inner.edit_brush.1.into());
            let _ = DeleteObject(inner.edit_font.1.into());
            return LRESULT(0);
        }
        let Ok(mut i) = cell.try_borrow_mut() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        let scale = scale_of(hwnd);
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = i.paint();
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_SIZE => i.relayout(),
            WM_TIMER if wp.0 == TIMER_SCROLL => {
                let d = i.target - i.scroll;
                let step = if d.abs() < 0.5 { d } else { d * 0.28 };
                let to = i.scroll + step;
                if (i.target - to).abs() < 0.01 {
                    let _ = KillTimer(Some(hwnd), TIMER_SCROLL);
                }
                i.scroll = to;
                i.place_edits();
                i.repaint();
                return LRESULT(0);
            }
            WM_TIMER if wp.0 == TIMER_ANIM => {
                // A leaving message is gone once it has faded out.
                if i.toast_out.is_some_and(|at| motion::progress(at, 0.0, TOAST_OUT_MS) >= 1.0) {
                    i.toast.clear();
                    i.toast_out = None;
                }
                if !i.animating() {
                    let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                }
                i.repaint();
                return LRESULT(0);
            }
            WM_TIMER if wp.0 == TIMER_TOAST => {
                let _ = KillTimer(Some(hwnd), TIMER_TOAST);
                // Fade the message out (at once when animations are off).
                if motion::enabled() {
                    i.toast_out = Some(Instant::now());
                    i.animate();
                } else {
                    i.toast.clear();
                }
                i.repaint();
                return LRESULT(0);
            }
            WM_MOUSEWHEEL => {
                let notches = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                let to = i.target - notches * WHEEL_STEP;
                i.scroll_smooth(to);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                if !i.mouse_in {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    i.mouse_in = true;
                    i.repaint();
                }
                let (x, y) = lparam_point(lp, scale);
                if let Some((y0, s0)) = i.bar_drag {
                    let (_, h) = i.size();
                    let (_, track) = i.bar_track(h);
                    let max = i.max_scroll();
                    let thumb = (track * track / (track + max)).max(32.0);
                    let to = s0 + (y - y0) * max / (track - thumb).max(1.0);
                    i.scroll_now(to);
                    return LRESULT(0);
                }
                let hover = i.target_at(x, y);
                let bar = hover == Some(Target::Bar);
                if hover != i.hover || bar != i.bar_hover {
                    if hover != i.hover {
                        i.hover_from = (hover.map_or(0.0, |t| i.hover_level(t)), i.hover.map_or(0.0, |t| i.hover_level(t)));
                        i.hover_prev = i.hover;
                        i.hover_at = Instant::now();
                        i.animate();
                    }
                    i.hover = hover;
                    i.bar_hover = bar;
                    i.repaint();
                }
            }
            WM_MOUSELEAVE => {
                i.mouse_in = false;
                i.bar_hover = false;
                if i.hover.is_some() {
                    i.hover_from = (0.0, i.hover.map_or(0.0, |t| i.hover_level(t)));
                    i.hover_prev = i.hover;
                    i.hover_at = Instant::now();
                    i.animate();
                }
                i.hover = None;
                i.repaint();
            }
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_point(lp, scale);
                let target = i.target_at(x, y);
                if let Some(t) = target.filter(|t| *t != Target::Bar) {
                    i.press = Some((t, Instant::now()));
                    i.animate();
                }
                if target == Some(Target::Bar) {
                    let scroll = i.scroll;
                    i.bar_drag = Some((y, scroll));
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                // Clicking the page takes the keyboard from a field (and keeps it for scrolling).
                if target != Some(Target::Search) {
                    let _ = SetFocus(Some(hwnd));
                }
                if let Some(t) = target {
                    drop(i);
                    // The dropdown menu runs a modal loop: Inner is borrowed again inside.
                    if let Ok(mut i) = cell.try_borrow_mut() {
                        i.click(t);
                        i.place_edits();
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP if i.bar_drag.is_some() => {
                i.bar_drag = None;
                let _ = ReleaseCapture();
                i.repaint();
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wp.0 as u16;
                if let Some(cmd) = shortcut(vk) {
                    i.send(cmd);
                } else if i.key(vk) {
                    return LRESULT(0);
                }
            }
            // The search box changed: filter the sections.
            WM_COMMAND if (wp.0 >> 16) as u32 == 0x0300 && HWND(lp.0 as _) == i.search => {
                i.filter = edit_text(i.search).trim().to_string();
                i.scroll = 0.0;
                i.target = 0.0;
                i.relayout();
                if i.doc.remote_search {
                    let cmd = format!("page\tsearch\t\t{}", i.filter);
                    i.send(cmd);
                }
            }
            // A form field changed: the browser keeps what was typed.
            WM_COMMAND if (wp.0 >> 16) as u32 == 0x0300 => {
                let edit = HWND(lp.0 as _);
                if let Some(id) = i.edits.iter().find(|(_, e)| *e == edit).map(|(id, _)| id.clone()) {
                    let form = id == super::FAKEBOX
                        || i.doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).any(|r| matches!(&r.control, Control::Text { id: t, button, .. } if *t == id && button.is_empty()));
                    if form {
                        let cmd = format!("page\tinput\t{id}\t{}", edit_text(edit));
                        i.send(cmd);
                    }
                }
            }
            WM_CTLCOLOREDIT => {
                let c = colors(i.dark);
                let hdc = HDC(wp.0 as _);
                SetTextColor(hdc, crate::frame::style::colorref(c.text));
                SetBkColor(hdc, crate::frame::style::colorref(c.field));
                if i.edit_brush.0 != c.field {
                    let _ = DeleteObject(i.edit_brush.1.into());
                    i.edit_brush = (c.field, CreateSolidBrush(crate::frame::style::colorref(c.field)));
                }
                return LRESULT(i.edit_brush.1.0 as isize);
            }
            _ => {}
        }
        drop(i);
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

/// Keys of the search box and the rows' text fields: Enter saves a field, Escape empties the
/// search; browser shortcuts are reported. Focus changes redraw the field outlines.
pub(super) unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    // SAFETY: `data` is the page's boxed RefCell<Inner>, alive while the page (our parent) is.
    unsafe {
        if crate::frame::edit_shortcut(hwnd, msg, wp) { return LRESULT(0); }
        let Some(cell) = (data as *const RefCell<Inner>).as_ref() else { return DefSubclassProc(hwnd, msg, wp, lp) };
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = wp.0 as u16;
                let Ok(mut i) = cell.try_borrow_mut() else { return DefSubclassProc(hwnd, msg, wp, lp) };
                if let Some(cmd) = shortcut(vk) {
                    i.send(cmd);
                }
                if vk == VK_ESCAPE.0 && GetKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT.0 as i32) < 0 { return LRESULT(0); }
                if i.fakebox_edit() == Some(hwnd) && (vk == VK_UP.0 || vk == VK_DOWN.0 || vk == VK_ESCAPE.0) {
                    let key = if vk == VK_UP.0 { "up" } else if vk == VK_DOWN.0 { "down" } else { "escape" };
                    i.send(format!("page\tkey\t{}\t{key}", super::FAKEBOX));
                    return LRESULT(0);
                }
                if vk == VK_ESCAPE.0 && hwnd == i.search {
                    drop(i);
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, w!(""));
                    return LRESULT(0);
                }
                if vk == VK_RETURN.0 {
                    if let Some(id) = i.edits.iter().find(|(_, e)| *e == hwnd).map(|(id, _)| id.clone()) {
                        let text = edit_text(hwnd);
                        i.send(format!("page\ttext\t{id}\t{text}"));
                    }
                    return LRESULT(0);
                }
            }
            // No beep for Enter / Escape.
            WM_CHAR if wp.0 == 13 || wp.0 == 27 => return LRESULT(0),
            WM_SETFOCUS | WM_KILLFOCUS => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                if let Ok(mut i) = cell.try_borrow_mut() {
                    if msg == WM_KILLFOCUS && i.fakebox_edit() == Some(hwnd) {
                        i.send(format!("page\tkey\t{}\tblur", super::FAKEBOX));
                    }
                    i.place_edits();
                    i.repaint();
                }
                return r;
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
