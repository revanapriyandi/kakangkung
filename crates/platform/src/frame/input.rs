//! Mouse and keyboard: the frame's window procedure and the address bar's subclass.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Gdi::{BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DeleteObject, EndPaint, FF_DONTCARE, HDC, InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SetBkColor, SetTextColor};
use windows::Win32::UI::Controls::{EM_SETMARGINS, EM_SETSEL, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, WM_TIMER, DefWindowProcW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GWLP_USERDATA, PostMessageW, SendMessageW, SetWindowLongPtrW, SetWindowTextW, WM_CHAR, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_SYSKEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, CREATESTRUCTW, WM_NCCREATE, WM_PAINT, WM_RBUTTONUP, WM_SETFOCUS, WM_SETFONT, WM_SIZE};
use windows::core::{w, HSTRING};

use super::style::{STRIP_H, colorref, palette};
use super::layout::{inside, Hit};
use super::Inner;

pub(crate) fn edit_text(edit: HWND) -> String {
    // SAFETY: the buffer is sized from GetWindowTextLengthW (+1 for the terminator).
    unsafe {
        let n = GetWindowTextLengthW(edit) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..got])
    }
}

pub(super) fn set_edit_text(i: &mut Inner, text: &str) {
    i.quiet = true;
    // SAFETY: live child control.
    unsafe {
        let _ = SetWindowTextW(i.edit, &HSTRING::from(text));
    }
    i.quiet = false;
}

pub(crate) fn scale_of(hwnd: HWND) -> f32 {
    // SAFETY: plain query for a live window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

pub(crate) fn lparam_point(l: LPARAM, scale: f32) -> (f32, f32) {
    let x = (l.0 & 0xffff) as i16 as f32 / scale;
    let y = ((l.0 >> 16) & 0xffff) as i16 as f32 / scale;
    (x, y)
}

impl Inner {
    pub(super) fn send(&mut self, cmd: String) {
        (self.on_command)(cmd);
    }

    pub(super) fn hit(&self, x: f32, y: f32) -> Hit {
        let h = self.hits.iter().rev().find(|(r, _)| inside(r, x, y)).map_or(Hit::Drag, |(_, h)| *h);
        if h != Hit::Drag || y >= STRIP_H {
            return h;
        }
        // Above a tab (or a button of the strip) up to the window's top edge counts as that tab,
        // like Chrome: a click at the very top of the screen still picks the tab under it.
        self.hits
            .iter()
            .rev()
            .find(|(r, h)| matches!(h, Hit::Tab(_) | Hit::Group(_) | Hit::NewTab | Hit::TabSearch) && x >= r.left && x < r.right && r.bottom <= STRIP_H && y < r.top)
            .map_or(Hit::Drag, |(_, h)| *h)
    }

    /// Mouse button released over `hit` (it was pressed there too).
    pub(super) fn click(&mut self, hit: Hit, x: f32, y: f32) {
        let r = |rect: D2D_RECT_F| (rect.right, rect.bottom, rect.left);
        let place = self.hits.iter().find(|(_, h)| *h == hit).map(|(rc, _)| r(*rc)).unwrap_or((x, y, x));
        let cmd = match hit {
            Hit::TabClose(id) => format!("close\t{id}"),
            Hit::TabAudio(id) => format!("mutetab\t{id}"),
            Hit::Group(id) => format!("groupcollapse\t{id}"),
            Hit::NewTab => "newtab".into(),
            Hit::TabSearch => "tabsearch".into(),
            Hit::Min => "min".into(),
            Hit::Max => "max".into(),
            Hit::Close => "quit".into(),
            Hit::Back => "back".into(),
            Hit::Forward => "forward".into(),
            Hit::Reload => if self.state.loading { "stop".into() } else { "reload".into() },
            Hit::Home => "home".into(),
            Hit::Star => "bookmark".into(),
            Hit::Key => format!("pwkey\t{}\t{}", place.0.round(), (place.1 + 6.0).round()),
            Hit::Extensions => format!("extensions\t{}\t{}", place.0.round(), (place.1 + 2.0).round()),
            Hit::Profile => format!("profile\t{}\t{}", place.0.round(), (place.1 + 2.0).round()),
            Hit::Menu => format!("menu\t{}\t{}", place.0.round(), (place.1 + 2.0).round()),
            Hit::Chip => format!("siteinfo\t{}\t{}", place.2.round(), (place.1 + 6.0).round()),
            Hit::Pinned(i) => match self.state.pinned.get(i) {
                Some(pin) => format!("pinned\t{}\t{}\t{}", pin.id, place.0.round(), (place.1 + 2.0).round()),
                None => return,
            },
            // Paint clamps the scroll to the strip.
            Hit::ScrollLeft | Hit::ScrollRight => {
                let page = 6.0 * super::style::TAB_MIN_W;
                self.scroll += if hit == Hit::ScrollLeft { -page } else { page };
                // SAFETY: repaint request for our own live window.
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
                return;
            }
            Hit::Tab(_) | Hit::Drag | Hit::None => return,
        };
        self.send(cmd);
    }
}

pub(super) unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY (whole function): runs on the UI thread for our own window; GWLP_USERDATA holds the
    // boxed RefCell<Inner> set in WM_NCCREATE and freed in WM_DESTROY.
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
            let inner = Box::from_raw(ptr);
            let i = inner.into_inner();
            let _ = DeleteObject(i.edit_font.into());
            let _ = DeleteObject(i.edit_brush.1.into());
            return LRESULT(0);
        }
        // A command handler may call back into the frame (set_state): never hold the borrow
        // while sending. `send` collects the command, which is delivered after the borrow ends.
        let Ok(mut i) = cell.try_borrow_mut() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        let scale = scale_of(hwnd);
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_TIMER if wp.0 == super::TIMER_ANIM => {
                if !i.appear.iter().any(|(_, at)| crate::motion::progress(*at, 0.0, super::TAB_GROW_MS) < 1.0) {
                    i.appear.clear();
                    let _ = KillTimer(Some(hwnd), super::TIMER_ANIM);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_TIMER if wp.0 == super::TIMER_SPIN => {
                if !i.state.tabs.iter().any(|t| t.loading) {
                    let _ = KillTimer(Some(hwnd), super::TIMER_SPIN);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            // The wheel over the tab strip scrolls it sideways when the tabs do not fit.
            WM_MOUSEWHEEL => {
                let mut pt = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                if (pt.y as f32 / scale) < STRIP_H {
                    let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                    i.scroll -= delta * 3.0 * super::style::TAB_MIN_W;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = i.paint();
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_SIZE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_MOUSEMOVE => {
                if !i.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    i.tracking = true;
                }
                let (x, y) = lparam_point(lp, scale);
                // Tab drag: move the tab where the pointer is.
                if let Some((id, start, moved)) = i.drag {
                    if moved || (x - start).abs() > 6.0 {
                        i.drag = Some((id, start, true));
                        let target = i.hits.iter().find_map(|(r, h)| match h {
                            Hit::Tab(other) if *other != id && inside(r, x, r.top + 1.0) => Some(*other),
                            _ => None,
                        });
                        if let Some(other) = target {
                            let to = i.state.tabs.iter().position(|t| t.id == other);
                            let from = i.state.tabs.iter().position(|t| t.id == id);
                            if let (Some(from), Some(to)) = (from, to) {
                                let tab = i.state.tabs.remove(from);
                                i.state.tabs.insert(to, tab);
                                i.send(format!("move\t{id}\t{to}"));
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        }
                    }
                }
                let h = i.hit(x, y);
                if h != i.hover {
                    i.hover = h;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            WM_MOUSELEAVE => {
                i.tracking = false;
                if i.hover != Hit::None {
                    i.hover = Hit::None;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_point(lp, scale);
                let h = i.hit(x, y);
                i.pressed = h;
                if y < 4.0 && !i.state.maximized {
                    let width = i.hits.iter().map(|(r, _)| r.right).fold(0.0f32, f32::max);
                    let dir = if x < 8.0 { "nw" } else if x > width - 8.0 { "ne" } else { "n" };
                    i.send(format!("resize\t{dir}"));
                    return LRESULT(0);
                }
                match h {
                    // Like Chrome: a tab is selected on press; dragging moves it.
                    Hit::Tab(id) => {
                        if id != i.state.active {
                            i.send(format!("select\t{id}"));
                        }
                        i.drag = Some((id, x, false));
                        SetCapture(hwnd);
                    }
                    // The window moves when the empty strip is dragged.
                    Hit::Drag if y < STRIP_H => i.send("drag".into()),
                    // Menus open on press, like Chrome.
                    Hit::Menu | Hit::Profile | Hit::Extensions | Hit::Chip | Hit::TabSearch => {
                        i.pressed = Hit::None;
                        i.click(h, x, y);
                    }
                    _ => {}
                }
            }
            WM_LBUTTONUP => {
                if i.drag.take().is_some() {
                    let _ = ReleaseCapture();
                }
                let (x, y) = lparam_point(lp, scale);
                let h = i.hit(x, y);
                if h == i.pressed {
                    i.click(h, x, y);
                }
                i.pressed = Hit::None;
            }
            WM_LBUTTONDBLCLK => {
                let (x, y) = lparam_point(lp, scale);
                if i.hit(x, y) == Hit::Drag && y < STRIP_H {
                    i.send("max".into());
                }
            }
            WM_MBUTTONUP => {
                let (x, y) = lparam_point(lp, scale);
                if let Hit::Tab(id) | Hit::TabClose(id) = i.hit(x, y) {
                    i.send(format!("close\t{id}"));
                }
            }
            WM_RBUTTONUP => {
                let (x, y) = lparam_point(lp, scale);
                match i.hit(x, y) {
                    Hit::Tab(id) | Hit::TabClose(id) | Hit::TabAudio(id) => i.send(format!("tabmenu\t{id}\t{}\t{}", x.round(), y.round())),
                    Hit::Group(id) => i.send(format!("groupmenu\t{id}\t{}\t{}", x.round(), y.round())),
                    Hit::Pinned(n) => {
                        if let Some(id) = i.state.pinned.get(n).map(|p| p.id.clone()) {
                            i.send(format!("pinnedmenu\t{id}\t{}\t{}", x.round(), y.round()));
                        }
                    }
                    _ => {}
                }
            }
            WM_COMMAND => {
                // EN_CHANGE from the address bar: typing (not our own SetWindowText).
                if (wp.0 >> 16) as u32 == 0x0300 && !i.quiet {
                    let text = edit_text(i.edit);
                    i.send(format!("omnichange\t{text}"));
                }
            }
            WM_CTLCOLOREDIT => {
                let p = palette(i.state.dark, i.state.incognito);
                let hdc = HDC(wp.0 as _);
                SetTextColor(hdc, colorref(p.text));
                SetBkColor(hdc, colorref(p.omni));
                if i.edit_brush.0 != p.omni {
                    let _ = DeleteObject(i.edit_brush.1.into());
                    i.edit_brush = (p.omni, CreateSolidBrush(colorref(p.omni)));
                }
                return LRESULT(i.edit_brush.1.0 as isize);
            }
            _ => {}
        }
        // Font of the address bar follows the DPI (created lazily, once per DPI).
        if i.edit_font.is_invalid() && !i.edit.is_invalid() {
            let font = CreateFontW(-(14.0 * scale) as i32, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"));
            i.edit_font = font;
            SendMessageW(i.edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            SendMessageW(i.edit, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM(0)));
        }
        drop(i);
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

/// Keys of the address bar: Enter, Escape, Up/Down are reported; the rest is normal editing.
pub(super) unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    // SAFETY: `data` is the frame's boxed RefCell<Inner>, alive while the frame (our parent) is.
    unsafe {
        let cell = (data as *const RefCell<Inner>).as_ref();
        let report = |cmd: String| {
            if let Some(Ok(mut i)) = cell.map(RefCell::try_borrow_mut) {
                i.send(cmd);
            }
        };
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let key = wp.0 as u16;
                // Browser shortcuts typed in the box (Ctrl+T, Alt+D, F5, ...) go to the browser;
                // the box keeps handling them too (Ctrl+A/C/V/X/Z are not browser shortcuts).
                let down = |vk: u16| GetKeyState(vk as i32) < 0;
                let (ctrl, shift, alt) = (down(VK_CONTROL.0), down(VK_SHIFT.0), down(VK_MENU.0));
                if ctrl || alt || (0x70..=0x7B).contains(&key) {
                    report(format!("key\t{key}\t{}\t{}\t{}", ctrl as u8, shift as u8, alt as u8));
                }
                let name = match key {
                    k if k == VK_RETURN.0 => Some("enter"),
                    k if k == VK_ESCAPE.0 => Some("escape"),
                    k if k == VK_UP.0 => Some("up"),
                    k if k == VK_DOWN.0 => Some("down"),
                    _ => None,
                };
                if let Some(name) = name {
                    report(format!("omnikey\t{name}"));
                    return LRESULT(0);
                }
            }
            // No beep for Enter / Escape.
            WM_CHAR if wp.0 == 13 || wp.0 == 27 => return LRESULT(0),
            WM_SETFOCUS => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                report("omnifocus".into());
                // Select everything once the click that focused us is done.
                let _ = PostMessageW(Some(hwnd), EM_SETSEL, WPARAM(0), LPARAM(-1));
                return r;
            }
            WM_KILLFOCUS => report("omniblur".into()),
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
