//! Native list pop-up above the page: address-bar suggestions and tab search. A borderless
//! window owned by the browser window (so it is always above the page), drawn with Direct2D in
//! the frame's style, with Windows 11 rounded corners and shadow.
//!
//! Suggestions never take the keyboard (the address bar keeps it). Tab search has its own filter
//! box and closes when it loses focus. Choices are reported as the browser's commands:
//! `pick\t<kind>\t<row id>`, `overlayclose`, `overlayhide`.

mod input;
mod paint;

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush, D2D1_FACTORY_TYPE_SINGLE_THREADED};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING,
};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::Graphics::Gdi::{ClientToScreen, InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetTimer, GetWindowLongPtrW, IsWindowVisible, SetForegroundWindow, SetWindowLongPtrW, LoadCursorW, RegisterClassExW, SetWindowPos, ShowWindow,
    CS_DROPSHADOW, ES_AUTOHSCROLL, GWLP_USERDATA, GWL_EXSTYLE, HWND_TOP, IDC_ARROW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WNDCLASSEXW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::frame::paint::{icon_family, text_format};
use crate::frame::scale_of;

/// One row of the list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PopupRow {
    /// Sent back when the row is chosen (an address, or a command id).
    pub id: String,
    /// Icon: "search", "history", "bookmark", "url" or "tab".
    pub kind: String,
    pub title: String,
    /// Shown after the title in the accent colour (an address), or on the right (a shortcut).
    pub detail: String,
    /// A second line under the title ("github.com • 3 mins ago"); such rows are taller and show
    /// a lettered site badge.
    pub sub: String,
    /// Picked when the row's × is clicked ("" = no ×).
    pub close: String,
}

/// Row height in DIPs per list style. Rows of kind "header" title a section; a header with an id
/// folds its section away when clicked.
const SUGGEST_ROW: f32 = 36.0;
const MENU_ROW: f32 = 32.0;
const HEADER_ROW: f32 = 40.0;
const TWO_LINE_ROW: f32 = 56.0;
const PADDING: f32 = 6.0;
const FILTER_H: f32 = 52.0;
/// Tallest list with a filter box (tab search), before it scrolls.
const FILTER_LIST_MAX: f32 = 600.0;
/// Width of a row's × at its right end.
const CLOSE_W: f32 = 36.0;
/// Opening animation: length at normal speed (ms) and the share of the height shown at once.
const REVEAL_MS: f32 = 170.0;
const REVEAL_FROM: f32 = 0.55;
const TIMER_REVEAL: usize = 1;

struct Inner {
    hwnd: HWND,
    owner: HWND,
    filter: HWND,
    /// Background brush of the filter box: (colour, brush).
    filter_brush: (u32, HBRUSH),
    /// Command kind of the rows on show ("suggest" or "menu").
    kind: String,
    rows: Vec<PopupRow>,
    /// Indexes of `rows` passing the filter.
    shown: Vec<usize>,
    selected: Option<usize>,
    hover: Option<usize>,
    scroll: usize,
    row_h: f32,
    dark: bool,
    factory: ID2D1Factory,
    target: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    text: IDWriteTextFormat,
    bold: IDWriteTextFormat,
    small: IDWriteTextFormat,
    /// Letters of the site badges.
    center: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    icons_small: IDWriteTextFormat,
    /// Sections folded away (header ids).
    collapsed: Vec<String>,
    /// The pointer is over the hovered row's ×.
    hover_close: bool,
    /// Opening: when, and the full height (px) the window grows to.
    reveal: Option<(std::time::Instant, i32)>,
    on_command: Box<dyn FnMut(String)>,
}

/// The pop-up window (hidden until `show`). Dropping it destroys the window.
pub struct Popup {
    hwnd: HWND,
}

impl Popup {
    /// Create the (hidden) pop-up owned by the browser window `owner`.
    pub fn create(owner: isize, on_command: impl FnMut(String) + 'static) -> windows::core::Result<Popup> {
        // SAFETY: window-class registration and window creation on the UI thread; the boxed
        // Inner belongs to the window (GWLP_USERDATA) until WM_DESTROY frees it.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(input::popup_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: w!("KakangkungPopup"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let dw = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icons = icon_family(&dw);
            let inner = Box::new(RefCell::new(Inner {
                hwnd: HWND::default(),
                owner: HWND(owner as _),
                filter: HWND::default(),
                filter_brush: (u32::MAX, HBRUSH::default()),
                kind: String::new(),
                rows: vec![],
                shown: vec![],
                selected: None,
                hover: None,
                scroll: 0,
                row_h: SUGGEST_ROW,
                dark: false,
                factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                target: None,
                brush: None,
                text: text_format(&dw, w!("Segoe UI"), 13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                bold: text_format(&dw, w!("Segoe UI"), 13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                small: text_format(&dw, w!("Segoe UI"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                collapsed: vec![],
                hover_close: false,
                reveal: None,
                icons: text_format(&dw, icons, 14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                icons_small: text_format(&dw, icons, 10.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                center: text_format(&dw, w!("Segoe UI"), 11.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                on_command: Box::new(on_command),
            }));
            let raw = Box::into_raw(inner);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("KakangkungPopup"),
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                Some(HWND(owner as _)),
                None,
                Some(instance.into()),
                Some(raw as *const c_void),
            )?;
            let filter = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                10,
                10,
                Some(hwnd),
                None,
                Some(instance.into()),
                None,
            )?;
            (*raw).borrow_mut().filter = filter;
            let _ = SetWindowSubclass(filter, Some(input::filter_proc), 1, raw as usize);
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const c_void, 4);
            Ok(Popup { hwnd })
        }
    }

    fn inner(&self) -> Option<&RefCell<Inner>> {
        // SAFETY: GWLP_USERDATA holds the boxed Inner from creation until WM_DESTROY.
        unsafe { (GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>).as_ref() }
    }

    /// Show `rows` at `x, y` (DIPs in the owner's client area), `width` DIPs wide. `kind` is the
    /// command kind reported on a pick. `filter` adds a type-to-filter box (tab search) and gives
    /// the pop-up the keyboard; without it the keyboard stays where it is (suggestions).
    #[allow(clippy::too_many_arguments)]
    pub fn show(&self, kind: &str, x: f32, y: f32, width: f32, rows: Vec<PopupRow>, selected: Option<usize>, filter: bool, dark: bool) {
        let Some(cell) = self.inner() else { return };
        let mut i = cell.borrow_mut();
        let scale = scale_of(i.owner);
        i.kind = kind.to_string();
        i.rows = rows;
        i.hover = None;
        i.scroll = 0;
        i.dark = dark;
        i.row_h = if kind == "suggest" { SUGGEST_ROW } else { MENU_ROW };
        i.rebuild("");
        // Tab search starts on its first entry; other lists as asked.
        i.selected = selected.or_else(|| if filter { i.first_item() } else { None });
        let list: f32 = i.shown.iter().map(|&k| i.height_of(k)).sum();
        let list = list.min(if filter { FILTER_LIST_MAX } else { 10.0 * i.row_h });
        let height = list + 2.0 * PADDING + if filter { FILTER_H } else { 0.0 };
        let mut pt = POINT { x: (x * scale).round() as i32, y: (y * scale).round() as i32 };
        // SAFETY: plain window calls on the live pop-up and its owner.
        unsafe {
            let _ = ClientToScreen(i.owner, &mut pt);
            // Suggestions never take the keyboard; tab search does (its filter box).
            let ex = GetWindowLongPtrW(i.hwnd, GWL_EXSTYLE);
            let no_activate = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(i.hwnd, GWL_EXSTYLE, if filter { ex & !no_activate } else { ex | no_activate });
            let flags = if filter { SWP_SHOWWINDOW } else { SWP_NOACTIVATE | SWP_SHOWWINDOW };
            // Opening (not a refresh while shown): it unfolds downwards from part of its height.
            let full = (height * scale) as i32;
            let opening = !IsWindowVisible(i.hwnd).as_bool() && crate::motion::enabled();
            let shown = if opening { (full as f32 * REVEAL_FROM) as i32 } else { full };
            i.reveal = opening.then(|| (std::time::Instant::now(), full));
            let _ = SetWindowPos(i.hwnd, Some(HWND_TOP), pt.x, pt.y, (width * scale) as i32, shown, flags);
            if opening {
                SetTimer(Some(i.hwnd), TIMER_REVEAL, crate::motion::FRAME_MS, None);
            }
            if filter {
                input::reset_filter(&mut i, scale, width);
                let (popup, edit) = (i.hwnd, i.filter);
                drop(i);
                let _ = SetForegroundWindow(popup);
                let _ = SetFocus(Some(edit));
            } else {
                let _ = ShowWindow(i.filter, SW_HIDE);
                let _ = InvalidateRect(Some(i.hwnd), None, false);
            }
        }
    }

    /// Mark row `index` as selected (keyboard selection in the address bar).
    pub fn highlight(&self, index: Option<usize>) {
        let Some(cell) = self.inner() else { return };
        let mut i = cell.borrow_mut();
        i.selected = index;
        // SAFETY: repaint request for the live pop-up.
        unsafe {
            let _ = InvalidateRect(Some(i.hwnd), None, false);
        }
    }

    pub fn hide(&self) {
        // SAFETY: plain call on our own window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: plain query on our own window.
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Rows currently listed (after the filter), for the self-test.
    pub fn shown_rows(&self) -> Vec<String> {
        self.inner().map_or(vec![], |c| {
            let i = c.borrow();
            i.shown.iter().map(|&k| i.rows[k].title.clone()).collect()
        })
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: destroying our own window; WM_DESTROY frees Inner.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
