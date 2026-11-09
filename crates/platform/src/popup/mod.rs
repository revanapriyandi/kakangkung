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

use windows::Win32::Foundation::COLORREF;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetLayeredWindowAttributes, LWA_ALPHA, SWP_NOMOVE, SWP_NOSIZE, WS_EX_LAYERED};
use windows::core::w;
use windows::Win32::Foundation::{RECT, HWND, POINT};
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
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, 
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
    /// Icon name ("search", "history", "settings", "check"… see `paint::icon`; "" = none), or
    /// the row's role: "header" (a section title), "note" (a line of text, not pickable), "sep"
    /// (a divider), "zoom" (Chrome's zoom row: `title`, then − `detail` + and full screen in
    /// the row, picked as `zoomout`, `zoomreset`, `zoomin`, `fullscreen`) or "buttons" (a dialog's buttons: `title` is the main button, picked as `id`;
    /// `detail` the other one, picked as `close`).
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
const BUTTONS_ROW: f32 = 56.0;
const SEP_ROW: f32 = 9.0;
/// A panel's plain lines: the first one (its title) and the others, which sit close together
/// like a paragraph (a long line arrives wrapped over several rows).
const NOTE_TITLE_ROW: f32 = 30.0;
const NOTE_ROW: f32 = 22.0;
/// A dialog's buttons: main and other button widths, and the gap between them.
const MAIN_BUTTON_W: f32 = 136.0;
const OTHER_BUTTON_W: f32 = 96.0;
const BUTTON_GAP: f32 = 8.0;
const PADDING: f32 = 6.0;
const FILTER_H: f32 = 52.0;
/// Tallest list with a filter box (tab search), before it scrolls.
const FILTER_LIST_MAX: f32 = 600.0;
/// Width of a row's × at its right end.
const CLOSE_W: f32 = 36.0;
/// Opening animation: length at normal speed (ms) and the share of the height shown at once.
const REVEAL_MS: f32 = 150.0;
/// How far (DIPs) an opening pop-up slides down into place as it fades in.
const REVEAL_SLIDE: f32 = 6.0;
const TIMER_REVEAL: usize = 1;
/// Closing: its timer, how long it takes and how far (DIPs) it lifts while fading out. Quicker
/// than opening: the person has already moved on.
const TIMER_CLOSE: usize = 3;
const CLOSE_MS: f32 = 100.0;
const CLOSE_SLIDE: f32 = 4.0;
/// Timer of the rows' hover fade, and how long it takes (ms at normal speed).
const TIMER_HOVER: usize = 2;
const HOVER_MS: f32 = 110.0;
/// A menu taking the keyboard back right after opening (see WM_KILLFOCUS), and how long after
/// opening that may happen.
/// A menu ("menu"), or a submenu opened beside it ("submenu"): rows of 28, the keyboard.
pub(super) fn is_menu(kind: &str) -> bool {
    kind == "menu" || kind == "submenu"
}

const WM_REFOCUS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
const REFOCUS_MS: u128 = 400;

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
    /// The row hovered before `hover`, and when the pointer moved: their highlights cross-fade.
    hover_prev: Option<usize>,
    hover_at: std::time::Instant,
    /// Strengths of (`hover`, `hover_prev`) when the pointer moved: the fades start from there.
    hover_from: (f32, f32),
    scroll: usize,
    row_h: f32,
    dark: bool,
    factory: ID2D1Factory,
    /// Kakangkung's own line icons, drawn for icon-font glyphs.
    vector: crate::icons::Icons,
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
    /// The pointer's x (DIPs) over the list, for the zoom row's buttons.
    mouse_x: f32,
    /// Opening: when, and the full height (px) the window grows to.
    reveal: Option<(std::time::Instant, i32)>,
    /// Closing: when, its opacity (0..=1) then, and its top (px) then. It takes no input meanwhile.
    closing: Option<(std::time::Instant, f32, i32)>,
    /// A menu with the keyboard (no filter box): losing it closes the menu.
    menu_focus: bool,
    /// When the menu on show opened.
    menu_opened: Option<std::time::Instant>,
    /// A text field only (no list): Enter reports `input\t<kind>\t<text>`.
    input_mode: bool,
    /// Shown in the empty field, and the hint at its right.
    placeholder: String,
    hint: String,
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
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
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
                hover_prev: None,
                hover_at: crate::motion::long_ago(),
                hover_from: (0.0, 0.0),
                scroll: 0,
                row_h: SUGGEST_ROW,
                dark: false,
                vector: crate::icons::Icons::new(&factory),
                factory,
                target: None,
                brush: None,
                text: text_format(&dw, w!("Segoe UI"), 13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                bold: text_format(&dw, w!("Segoe UI"), 13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                small: text_format(&dw, w!("Segoe UI"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                collapsed: vec![],
                hover_close: false,
                mouse_x: 0.0,
                reveal: None,
                closing: None,
                input_mode: false,
                menu_focus: false,
                menu_opened: None,
                placeholder: "Search Tabs".into(),
                hint: "Ctrl+Shift+A".into(),
                icons: text_format(&dw, icons, 14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                icons_small: text_format(&dw, icons, 10.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                center: text_format(&dw, w!("Segoe UI"), 11.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                on_command: Box::new(on_command),
            }));
            let raw = Box::into_raw(inner);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
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
        i.input_mode = false;
        i.placeholder = "Search Tabs".into();
        i.hint = "Ctrl+Shift+A".into();
        i.hover = None;
        i.hover_prev = None;
        i.scroll = 0;
        i.dark = dark;
        i.row_h = match kind {
            "suggest" => SUGGEST_ROW,
            // A long menu (⋮) fits a normal window whole at this height.
            k if is_menu(k) && !filter => MENU_ROW - 4.0,
            _ => MENU_ROW,
        };
        i.rebuild("");
        // Tab search starts on its first entry; other lists as asked.
        i.selected = selected.or_else(|| if filter { i.first_item() } else { None });
        let list: f32 = i.shown.iter().map(|&k| i.height_of(k)).sum();
        // A dialog (it has buttons) is shown whole; other lists scroll past ten rows.
        let dialog = i.rows.iter().any(|r| r.kind == "buttons");
        // Menus are shown whole as far as the window lets them (the rest scrolls); suggestion
        // lists stop at ten rows.
        let room = {
            let mut rc = RECT::default();
            // SAFETY: plain query on the owner window.
            let _ = unsafe { GetClientRect(i.owner, &mut rc) };
            ((rc.bottom - rc.top) as f32 / scale - y - 2.0 * PADDING - 8.0).max(200.0)
        };
        let list = if dialog {
            list
        } else if is_menu(kind) && !filter {
            list.min(room)
        } else {
            list.min(if filter { FILTER_LIST_MAX } else { 10.0 * i.row_h })
        };
        let height = list + 2.0 * PADDING + if filter { FILTER_H } else { 0.0 };
        let mut pt = POINT { x: (x * scale).round() as i32, y: (y * scale).round() as i32 };
        // SAFETY: plain window calls on the live pop-up and its owner.
        unsafe {
            let _ = ClientToScreen(i.owner, &mut pt);
            // Suggestions never take the keyboard; tab search (its filter box) and menus do, so
            // arrows, Enter and Escape work and a click anywhere else closes them.
            let activate = filter || is_menu(kind);
            i.menu_focus = is_menu(kind) && !filter;
            if i.menu_focus && !IsWindowVisible(i.hwnd).as_bool() {
                i.menu_opened = Some(std::time::Instant::now());
            }
            let ex = GetWindowLongPtrW(i.hwnd, GWL_EXSTYLE);
            let no_activate = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(i.hwnd, GWL_EXSTYLE, if activate { ex & !no_activate } else { ex | no_activate });
            let flags = if activate { SWP_SHOWWINDOW } else { SWP_NOACTIVATE | SWP_SHOWWINDOW };
            // Opening (not a refresh while shown): at its full size from the start, it fades in
            // while sliding a few pixels down into place. Moving a window is cheap for Windows to
            // compose; resizing it every frame would also resize and redraw the list.
            let full = (height * scale) as i32;
            // Reopened while it fades out: it fades back in from its current opacity.
            let resumed = i.closing.take().map(|_| i.alpha_level());
            if resumed.is_some() {
                let _ = KillTimer(Some(i.hwnd), TIMER_CLOSE);
                input::set_click_through(i.hwnd, false);
            }
            let hidden = !IsWindowVisible(i.hwnd).as_bool();
            let opening = (hidden || resumed.is_some()) && crate::motion::enabled();
            // Opening starts a little visible, so the first frame on screen is not an empty step.
            let from = resumed.unwrap_or(0.0).max(crate::motion::FADE_FROM);
            let start = crate::motion::fade_start_at(from, REVEAL_MS);
            let lift = if opening { ((1.0 - crate::motion::eased(start, 0.0, REVEAL_MS)) * REVEAL_SLIDE * scale).round() as i32 } else { 0 };
            i.reveal = opening.then_some((start, pt.y));
            // A pop-up shown without the animation is opaque.
            if opening || hidden || resumed.is_some() {
                let alpha = if opening { (from * 255.0).round() as u8 } else { 255 };
                let _ = SetLayeredWindowAttributes(i.hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            // Placed and drawn while still hidden, then shown: the first frame on screen is complete.
            let _ = SetWindowPos(i.hwnd, Some(HWND_TOP), pt.x, pt.y - lift, (width * scale) as i32, full, (flags & !SWP_SHOWWINDOW) | SWP_NOACTIVATE);
            if hidden {
                let _ = i.paint();
            }
            let _ = SetWindowPos(i.hwnd, Some(HWND_TOP), 0, 0, 0, 0, flags | SWP_NOMOVE | SWP_NOSIZE);
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
                if activate {
                    let popup = i.hwnd;
                    drop(i);
                    let _ = SetForegroundWindow(popup);
                    let _ = SetFocus(Some(popup));
                }
            }
        }
    }

    /// A one-line text field at `x, y` (DIPs in the owner's client area), `width` DIPs wide,
    /// holding `text` (selected) with `placeholder` when empty. It takes the keyboard; Enter
    /// reports `input\t<kind>\t<text>`, Escape or a click elsewhere closes it.
    #[allow(clippy::too_many_arguments)]
    pub fn show_input(&self, kind: &str, x: f32, y: f32, width: f32, text: &str, placeholder: &str, dark: bool) {
        self.show(kind, x, y, width, vec![], None, true, dark);
        let Some(cell) = self.inner() else { return };
        let mut i = cell.borrow_mut();
        i.input_mode = true;
        i.placeholder = placeholder.into();
        i.hint = "Enter".into();
        let scale = scale_of(i.owner);
        let height = ((FILTER_H + 2.0 * PADDING - 6.0) * scale) as i32;
        let (hwnd, edit) = (i.hwnd, i.filter);
        i.reveal = None;
        drop(i);
        // SAFETY: plain calls on our own live windows.
        unsafe {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rc);
            let _ = SetWindowPos(hwnd, None, 0, 0, rc.right - rc.left, height, windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(edit, &windows::core::HSTRING::from(text));
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(edit, windows::Win32::UI::Controls::EM_SETSEL, Some(windows::Win32::Foundation::WPARAM(0)), Some(windows::Win32::Foundation::LPARAM(-1)));
            let _ = InvalidateRect(Some(hwnd), None, false);
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

    /// Where the row `id` is: (left, right, top) in DIPs of the owner's client area (a submenu
    /// opens beside it).
    pub fn row_box(&self, id: &str) -> Option<(f32, f32, f32)> {
        let i = self.inner()?.borrow();
        let slot = i.shown.iter().position(|&k| i.rows[k].id == id)?;
        let top = i.list_top() + i.shown[i.scroll.min(slot)..slot].iter().map(|&k| i.height_of(k)).sum::<f32>();
        let scale = scale_of(i.owner);
        let mut rc = RECT::default();
        // SAFETY: plain queries on our own window and its owner.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(i.hwnd, &mut rc);
            let mut pts = [POINT { x: rc.left, y: rc.top }, POINT { x: rc.right, y: rc.bottom }];
            windows::Win32::Graphics::Gdi::MapWindowPoints(None, Some(i.owner), &mut pts);
            Some((pts[0].x as f32 / scale, pts[1].x as f32 / scale, pts[0].y as f32 / scale + top))
        }
    }

    /// Give the keyboard to the pop-up on show (back from its submenu).
    pub fn focus(&self) {
        // SAFETY: plain calls on our own window.
        unsafe {
            if IsWindowVisible(self.hwnd).as_bool() {
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
        }
    }

    /// Close it: it fades out while lifting a little (taking no clicks meanwhile), then hides.
    /// It counts as closed at once; reopening it during the fade picks up from there.
    pub fn hide(&self) {
        // SAFETY: plain calls on our own window.
        unsafe {
            let fade = IsWindowVisible(self.hwnd).as_bool() && crate::motion::enabled();
            let Some(Ok(mut i)) = self.inner().map(RefCell::try_borrow_mut).filter(|_| fade) else {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                return;
            };
            if i.closing.is_some() {
                return;
            }
            let level = i.alpha_level();
            i.reveal = None;
            let _ = KillTimer(Some(self.hwnd), TIMER_REVEAL);
            let mut rc = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut rc);
            i.closing = Some((std::time::Instant::now(), level, rc.top));
            i.menu_focus = false;
            input::set_click_through(self.hwnd, true);
            // The keyboard goes back to the browser now, not after the fade.
            if windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == self.hwnd {
                let _ = SetForegroundWindow(i.owner);
            }
            SetTimer(Some(self.hwnd), TIMER_CLOSE, crate::motion::FRAME_MS, None);
        }
    }

    /// Shown and not on its way out.
    pub fn is_visible(&self) -> bool {
        let closing = self.inner().and_then(|c| c.try_borrow().ok().map(|i| i.closing.is_some())).unwrap_or(false);
        // SAFETY: plain query on our own window.
        unsafe { IsWindowVisible(self.hwnd).as_bool() && !closing }
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
