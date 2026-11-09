//! Controls of the docked DevTools panel, which WebView2's inspector window does not have when
//! hosted: a rail of buttons (dock to the bottom, right or left, close) and a splitter that
//! resizes the panel. Two small child windows of the browser window, drawn with Direct2D.

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, InvalidateRect, ScreenToClient, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetWindowLongPtrW, LoadCursorW, RegisterClassExW, SetCursor, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, CREATESTRUCTW, GWLP_USERDATA, HTCLIENT, HWND_TOP, IDC_ARROW, IDC_SIZENS, IDC_SIZEWE, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA, WINDOW_EX_STYLE, WM_DESTROY,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS,
};

use crate::frame::layout::rect;
use crate::frame::scale_of;
use crate::frame::style::{color, glyph};

/// The rail's buttons, top to bottom: (command, icon). ⋮ opens the menu of dock positions.
const BUTTONS: [(&str, &str); 2] = [("dockmenu", glyph::MORE), ("dockclose", glyph::CLOSE)];
/// Width of the rail and of the splitter (DIPs), and the rail's button size.
pub const RAIL_W: f64 = 34.0;
pub const SPLIT_W: f64 = 5.0;
const BUTTON: f32 = 28.0;

#[derive(PartialEq)]
enum Kind {
    Rail,
    Splitter,
}

struct Inner {
    kind: Kind,
    hwnd: HWND,
    parent: HWND,
    /// "bottom", "right" or "left": the rail lights that button; the splitter's direction.
    side: String,
    dark: bool,
    hover: Option<usize>,
    dragging: bool,
    factory: ID2D1Factory,
    vector: crate::icons::Icons,
    target: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    on_command: Box<dyn FnMut(String)>,
}

/// The rail and splitter of the docked DevTools. Dropping it destroys both windows.
pub struct DockControls {
    rail: HWND,
    splitter: HWND,
}

impl DockControls {
    /// Create both (hidden) as children of the browser window `parent`. `on_command` receives
    /// `dockside\t<side>`, `dockclose` and, while the splitter is dragged, `docksplit\t<x>\t<y>`
    /// (the pointer in `parent`'s client area, physical pixels).
    pub fn create(parent: isize, on_command: impl FnMut(String) + Clone + 'static) -> windows::core::Result<Self> {
        let parent = HWND(parent as _);
        // SAFETY: class registration and child-window creation on the UI thread; each `Inner`
        // is boxed and owned by its window (GWLP_USERDATA) until WM_DESTROY frees it.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: w!("KakangkungDock"),
                ..Default::default()
            };
            RegisterClassExW(&class); // fails harmlessly when already registered
            let make = |kind: Kind, on_command: Box<dyn FnMut(String)>| -> windows::core::Result<HWND> {
                let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
                let inner = Box::new(RefCell::new(Inner {
                    kind,
                    hwnd: HWND::default(),
                    parent,
                    side: "bottom".into(),
                    dark: false,
                    hover: None,
                    dragging: false,
                    vector: crate::icons::Icons::new(&factory),
                    factory,
                    target: None,
                    brush: None,
                    on_command,
                }));
                let raw = Box::into_raw(inner);
                CreateWindowExW(WINDOW_EX_STYLE(0), w!("KakangkungDock"), w!(""), WS_CHILD | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(parent), None, Some(instance.into()), Some(raw as *const c_void))
                    .inspect_err(|_| drop(Box::from_raw(raw)))
            };
            let rail = make(Kind::Rail, Box::new(on_command.clone()))?;
            let splitter = match make(Kind::Splitter, Box::new(on_command)) {
                Ok(h) => h,
                Err(e) => {
                    let _ = DestroyWindow(rail);
                    return Err(e);
                }
            };
            Ok(DockControls { rail, splitter })
        }
    }

    /// Show the rail and splitter at these physical client rectangles (x, y, width, height).
    pub fn place(&self, rail: [i32; 4], splitter: [i32; 4], side: &str, dark: bool) {
        for (hwnd, [x, y, w, h]) in [(self.rail, rail), (self.splitter, splitter)] {
            if let Some(cell) = inner(hwnd) {
                if let Ok(mut i) = cell.try_borrow_mut() {
                    i.side = side.to_string();
                    i.dark = dark;
                }
            }
            // SAFETY: our own live child windows.
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w.max(1), h.max(1), SWP_NOACTIVATE);
                let _ = ShowWindow(hwnd, SW_SHOWNA);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }

    pub fn hide(&self) {
        // SAFETY: our own live child windows.
        unsafe {
            let _ = ShowWindow(self.rail, SW_HIDE);
            let _ = ShowWindow(self.splitter, SW_HIDE);
        }
    }
}

impl Drop for DockControls {
    fn drop(&mut self) {
        // SAFETY: our own windows; WM_DESTROY frees their state.
        unsafe {
            let _ = DestroyWindow(self.rail);
            let _ = DestroyWindow(self.splitter);
        }
    }
}

fn inner<'a>(hwnd: HWND) -> Option<&'a RefCell<Inner>> {
    // SAFETY: GWLP_USERDATA holds the boxed Inner from creation until WM_DESTROY.
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Inner>).as_ref() }
}

/// Where button `n` of the rail sits (DIPs), for a rail `width` wide.
fn button_rect(n: usize, width: f32) -> D2D_RECT_F {
    rect((width - BUTTON) / 2.0, 6.0 + n as f32 * (BUTTON + 4.0), BUTTON, BUTTON)
}

impl Inner {
    fn button_at(&self, x: f32, y: f32, width: f32) -> Option<usize> {
        (0..BUTTONS.len()).find(|&n| {
            let r = button_rect(n, width);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })
    }

    fn paint(&mut self) -> windows::core::Result<()> {
        let scale = scale_of(self.hwnd);
        let mut rc = RECT::default();
        // SAFETY: Direct2D drawing on our own window between BeginDraw and EndDraw.
        unsafe {
            GetClientRect(self.hwnd, &mut rc)?;
            let size = D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 };
            if self.target.is_none() {
                let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE, dpiX: 96.0 * scale, dpiY: 96.0 * scale, ..Default::default() };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
                let t = self.factory.CreateHwndRenderTarget(&props, &hp)?;
                t.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                self.brush = Some(t.CreateSolidColorBrush(&color(0, 1.0), None)?);
                self.target = Some(t);
            }
            let (Some(t), Some(b)) = (self.target.clone(), self.brush.clone()) else { return Ok(()) };
            t.Resize(&size)?;
            t.SetDpi(96.0 * scale, 96.0 * scale);
            let (w, h) = (size.width as f32 / scale, size.height as f32 / scale);
            // DevTools' own colours, so the rail reads as part of the panel.
            let (bg, line, fg, hover, brand) = if self.dark {
                (0x282828, 0x3c3c3c, 0xc7c7c7, (0xffffff, 0.10), 0xa78bfa)
            } else {
                (0xf3f3f3, 0xd6d6d6, 0x474747, (0x000000, 0.08), 0x7c3aed)
            };
            t.BeginDraw();
            t.Clear(Some(&color(bg, 1.0)));
            match self.kind {
                Kind::Splitter => {
                    // A hairline in the middle; brand-coloured while pointed at or dragged.
                    let lit = self.hover.is_some() || self.dragging;
                    b.SetColor(&color(if lit { brand } else { line }, 1.0));
                    let thick = if lit { 2.0 } else { 1.0 };
                    let r = if self.side == "bottom" { rect(0.0, (h - thick) / 2.0, w, thick) } else { rect((w - thick) / 2.0, 0.0, thick, h) };
                    t.FillRectangle(&r, &b);
                }
                Kind::Rail => {
                    // Its edge towards the panel.
                    b.SetColor(&color(line, 1.0));
                    let edge = if self.side == "left" { rect(w - 1.0, 0.0, 1.0, h) } else { rect(0.0, 0.0, 1.0, h) };
                    t.FillRectangle(&edge, &b);
                    for (n, (_, icon)) in BUTTONS.iter().enumerate() {
                        let r = button_rect(n, w);
                        if self.hover == Some(n) {
                            b.SetColor(&color(hover.0, hover.1));
                            t.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 6.0, radiusY: 6.0 }, &b);
                        }
                        b.SetColor(&color(fg, 1.0));
                        self.vector.draw(&t, &b, icon, r, 18.0, 1.5);
                    }
                }
            }
            if t.EndDraw(None, None).is_err() {
                self.target = None;
            }
        }
        Ok(())
    }

    /// The pointer in the parent's client area (physical pixels).
    fn pointer_in_parent(&self) -> (i32, i32) {
        let mut pt = POINT::default();
        // SAFETY: plain cursor and coordinate queries on live windows.
        unsafe {
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(self.parent, &mut pt);
        }
        (pt.x, pt.y)
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds our boxed RefCell<Inner> between WM_NCCREATE and WM_DESTROY.
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
        if msg == WM_DESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            return LRESULT(0);
        }
        let Some(cell) = ptr.as_ref() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        let Ok(mut i) = cell.try_borrow_mut() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        let scale = scale_of(hwnd);
        let local = || ((lp.0 & 0xffff) as i16 as f32 / scale, ((lp.0 >> 16) & 0xffff) as i16 as f32 / scale);
        let width = || {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            (rc.right - rc.left) as f32 / scale
        };
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = i.paint();
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT && i.kind == Kind::Splitter => {
                let cursor = if i.side == "bottom" { IDC_SIZENS } else { IDC_SIZEWE };
                if let Ok(c) = LoadCursorW(None, cursor) {
                    SetCursor(Some(c));
                }
                return LRESULT(1);
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let hover = match i.kind {
                    Kind::Rail => {
                        let (x, y) = local();
                        i.button_at(x, y, width())
                    }
                    Kind::Splitter => Some(0),
                };
                if i.dragging {
                    let (x, y) = i.pointer_in_parent();
                    (i.on_command)(format!("docksplit\t{x}\t{y}"));
                }
                if hover != i.hover {
                    i.hover = hover;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                i.hover = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_LBUTTONDOWN if i.kind == Kind::Splitter => {
                i.dragging = true;
                SetCapture(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_LBUTTONUP if i.kind == Kind::Splitter => {
                if std::mem::take(&mut i.dragging) {
                    (i.on_command)("dockdone".into());
                }
                let _ = ReleaseCapture();
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let (x, y) = local();
                // (Commands are queued to the browser's event loop, never run inside this call.)
                match i.button_at(x, y, width()) {
                    // ⋮: the menu opens under the button, towards the page.
                    Some(0) => {
                        let r = button_rect(0, width());
                        let edge = if i.side == "left" { r.left } else { r.right };
                        let mut pt = POINT { x: (edge * scale) as i32, y: (r.bottom * scale) as i32 };
                        let _ = ClientToScreen(hwnd, &mut pt);
                        (i.on_command)(format!("dockmenu\t{}\t{}", pt.x, pt.y));
                    }
                    Some(n) => (i.on_command)(BUTTONS[n].0.to_string()),
                    None => {}
                }
                return LRESULT(0);
            }
            _ => {}
        }
        drop(i);
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_buttons_do_not_overlap() {
        for n in 1..BUTTONS.len() {
            assert!(button_rect(n, RAIL_W as f32).top >= button_rect(n - 1, RAIL_W as f32).bottom);
        }
        assert!(BUTTONS.iter().all(|(cmd, _)| cmd.starts_with("dock")));
    }
}
