//! The native browser frame: tab strip, toolbar and window buttons drawn with Direct2D and
//! DirectWrite in a child window at the top of the browser window, plus the address bar as a
//! native EDIT control. Icons come from the Windows icon font (Segoe Fluent Icons on Windows 11,
//! Segoe MDL2 Assets on Windows 10).
//!
//! The frame only draws [`FrameState`] and reports input as commands (the same strings the old
//! HTML tab strip sent: `select\t<id>`, `close\t<id>`, `nav\t<text>`, ...). It decides nothing.

mod input;
pub(crate) mod layout;
pub(crate) mod paint;
pub(crate) mod style;
pub(crate) use self::input::{edit_text, lparam_point, scale_of};

use std::cell::RefCell;
use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::{D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::{DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat};
use windows::Win32::Graphics::Gdi::{HBRUSH, HFONT, InvalidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, CreateWindowExW, CS_DBLCLKS, DestroyWindow, ES_AUTOHSCROLL, GetWindowLongPtrW, GWLP_USERDATA, IDC_ARROW, LoadCursorW, MoveWindow, RegisterClassExW, SendMessageW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_VISIBLE};
use windows::core::w;

use self::layout::{rect, Hit};
use self::paint::{icon_family, text_format};
use self::input::{edit_proc, frame_proc, set_edit_text};

pub use self::style::{FRAME_H, STRIP_H, TOOLBAR_H};

/// One tab as the strip draws it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameTab {
    pub id: u64,
    pub title: String,
    pub loading: bool,
    pub audio: bool,
    pub muted: bool,
    pub pinned: bool,
    pub private: bool,
    pub discarded: bool,
    /// Group colour as 0xRRGGBB, 0 = no group.
    pub group_color: u32,
    pub in_split: bool,
    /// Address of the site icon (drawn once loaded; a globe until then).
    pub favicon: String,
}

/// A tab group's label in the strip, drawn before its first tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameGroup {
    pub id: u32,
    pub name: String,
    pub color: u32,
    pub collapsed: bool,
}

/// Site security chip at the start of the address bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Chip {
    #[default]
    Internal,
    Secure,
    Insecure,
    Local,
}

/// A button pinned to the toolbar: an extension or a built-in page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pinned {
    /// Sent back as `pinned <id> <right> <bottom>` (tab-separated) when clicked.
    pub id: String,
    /// Icon-font glyph, drawn when `icon` is empty or not loaded.
    pub glyph: &'static str,
    /// Key of a loaded icon (`platform::load_icon_file`), "" for none.
    pub icon: String,
}

/// Everything the frame draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameState {
    pub dark: bool,
    pub incognito: bool,
    pub tabs: Vec<FrameTab>,
    /// Group of each tab id, for labels: (first tab id of the group, group).
    pub groups: Vec<(u64, FrameGroup)>,
    pub active: u64,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    /// Address shown while the address bar is not being edited.
    pub display: String,
    /// Address put in the box when editing starts.
    pub url: String,
    pub chip: Chip,
    pub bookmarked: bool,
    pub show_star: bool,
    /// The password manager's key in the address bar: the page has saved logins, or a login
    /// waits to be saved.
    pub key: bool,
    pub show_home: bool,
    pub show_extensions: bool,
    pub show_tab_search: bool,
    /// Buttons pinned to the toolbar, left to right, between the address bar and the extensions button.
    pub pinned: Vec<Pinned>,
    /// Account initial ("" = signed out).
    pub avatar: String,
    pub avatar_image: String,
    pub synced: bool,
    pub maximized: bool,
    /// Tab search is open (its button is drawn pressed).
    pub tab_search_open: bool,
}

/// Frame timer of animations, and how long a new tab takes to widen (ms at normal speed).
pub(super) const TIMER_ANIM: usize = 1;
pub(super) const TAB_GROW_MS: f32 = 200.0;
/// Timer of the loading spinners, and its interval: 20 frames a second is smooth enough for a
/// spinner and keeps the CPU-drawn frame cheap while pages load.
pub(super) const TIMER_SPIN: usize = 2;
const SPIN_MS: u32 = 50;
pub(super) const TIMER_TAB_PREVIEW: usize = 3;

struct Gfx {
    factory: ID2D1Factory,
    target: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    /// Site icons made for this target, by address.
    favicons: std::collections::HashMap<String, windows::Win32::Graphics::Direct2D::ID2D1Bitmap>,
    /// Loading spinner arcs (three quarters of a circle round the origin), by radius.
    spinners: Vec<(f32, windows::Win32::Graphics::Direct2D::ID2D1PathGeometry)>,
    text: IDWriteTextFormat,
    text_bold: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    icons_small: IDWriteTextFormat,
}

struct Inner {
    hwnd: HWND,
    edit: HWND,
    edit_font: HFONT,
    edit_brush: (u32, HBRUSH),
    state: FrameState,
    hover: Hit,
    pressed: Hit,
    preview_tab: Option<u64>,
    synthetic_hover: bool,
    synthetic_omnibox_focus: bool,
    tracking: bool,
    /// Tab drag: (tab id, start x, moved).
    drag: Option<(u64, f32, bool)>,
    /// Setting the box's text ourselves must not report it as typing.
    quiet: bool,
    gfx: Gfx,
    on_command: Box<dyn FnMut(String)>,
    /// Layout of the last paint: what is where (DIPs).
    hits: Vec<(D2D_RECT_F, Hit)>,
    /// Tabs opened lately (they widen into place), and when.
    appear: Vec<(u64, std::time::Instant)>,
    omni_rect: D2D_RECT_F,
    /// How far the tab strip is scrolled (DIPs) when its tabs do not fit.
    scroll: f32,
    /// Scroll the selected tab into view at the next paint.
    reveal: bool,
}

/// The frame window. Dropping it destroys the window.
pub struct Frame {
    hwnd: HWND,
}

impl Frame {
    /// Create the frame as a child of `parent` (the browser window). `on_command` receives the
    /// commands; it runs on the UI thread, inside the frame's window procedure.
    pub fn create(parent: isize, on_command: impl FnMut(String) + 'static) -> windows::core::Result<Frame> {
        // SAFETY: standard window-class registration and child-window creation on the UI thread.
        // `Inner` is boxed and owned by the window (GWLP_USERDATA) until WM_DESTROY frees it.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(frame_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: w!("KakangkungFrame"),
                ..Default::default()
            };
            RegisterClassExW(&class); // fails harmlessly when already registered (second window)
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icons = icon_family(&dw);
            let gfx = Gfx {
                factory,
                target: None,
                brush: None,
                favicons: Default::default(),
                spinners: vec![],
                text: text_format(&dw, w!("Segoe UI"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                text_bold: text_format(&dw, w!("Segoe UI"), 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                icons: text_format(&dw, icons, 16.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                icons_small: text_format(&dw, icons, 10.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            };
            let inner = Box::new(RefCell::new(Inner {
                hwnd: HWND::default(),
                edit: HWND::default(),
                edit_font: HFONT::default(),
                edit_brush: (u32::MAX, HBRUSH::default()),
                state: FrameState::default(),
                hover: Hit::None,
                pressed: Hit::None,
                preview_tab: None,
                synthetic_hover: false,
                synthetic_omnibox_focus: false,
                tracking: false,
                drag: None,
                quiet: false,
                gfx,
                on_command: Box::new(on_command),
                hits: vec![],
                omni_rect: rect(0.0, 0.0, 0.0, 0.0),
                appear: vec![],
                scroll: 0.0,
                reveal: true,
            }));
            let raw = Box::into_raw(inner);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("KakangkungFrame"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                100,
                100,
                Some(HWND(parent as _)),
                None,
                Some(instance.into()),
                Some(raw as *const c_void),
            )?;
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                10,
                10,
                Some(hwnd),
                None,
                Some(instance.into()),
                None,
            )?;
            {
                let mut i = (*raw).borrow_mut();
                i.edit = edit;
            }
            let _ = SetWindowSubclass(edit, Some(edit_proc), 1, raw as usize);
            Ok(Frame { hwnd })
        }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.0 as isize
    }

    fn inner(&self) -> Option<&RefCell<Inner>> {
        // SAFETY: GWLP_USERDATA holds the boxed Inner from creation until WM_DESTROY.
        unsafe {
            let p = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>;
            p.as_ref()
        }
    }

    /// Position in the parent's client area, physical pixels.
    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        // SAFETY: the frame window is alive while `self` is.
        unsafe {
            let _ = MoveWindow(self.hwnd, x, y, width, height, true);
        }
    }

    /// Draw this state. The address bar text follows it unless the person is editing it.
    pub fn set_state(&self, state: FrameState) {
        let Some(inner) = self.inner() else { return };
        let mut i = inner.borrow_mut();
        if i.state == state {
            return;
        }
        // SAFETY: the EDIT control is a live child of the frame.
        let editing = i.synthetic_omnibox_focus || unsafe { GetFocus() } == i.edit;
        let text_changed = i.state.display != state.display;
        // Tabs that were not there before widen into place (not the first tabs of the window).
        if !i.state.tabs.is_empty() {
            let now = std::time::Instant::now();
            let new: Vec<u64> = state.tabs.iter().map(|t| t.id).filter(|id| !i.state.tabs.iter().any(|t| t.id == *id)).collect();
            i.appear.retain(|(_, at)| crate::motion::progress(*at, 0.0, TAB_GROW_MS) < 1.0);
            if !new.is_empty() {
                i.appear.extend(new.into_iter().map(|id| (id, now)));
                // SAFETY: frame timer on our own live window.
                unsafe {
                    SetTimer(Some(i.hwnd), TIMER_ANIM, crate::motion::FRAME_MS, None);
                }
            }
        }
        if i.state.active != state.active || i.state.tabs.len() != state.tabs.len() {
            i.reveal = true;
        }
        // Spinners turn while any tab loads (they stand still when animations are off).
        // SAFETY: frame timer on our own live window.
        unsafe {
            if crate::motion::enabled() && state.tabs.iter().any(|t| t.loading) {
                SetTimer(Some(i.hwnd), TIMER_SPIN, SPIN_MS, None);
            } else {
                let _ = KillTimer(Some(i.hwnd), TIMER_SPIN);
            }
        }
        i.state = state;
        if text_changed && !editing {
            let display = i.state.display.clone();
            set_edit_text(&mut i, &display);
        }
        // SAFETY: repaint request for a live window.
        unsafe {
            let _ = InvalidateRect(Some(i.hwnd), None, false);
        }
    }

    /// Put the keyboard in the address bar, full address selected (Ctrl+L).
    pub fn focus_omnibox(&self) {
        let Some(inner) = self.inner() else { return };
        let edit = inner.borrow().edit;
        // SAFETY: live child control.
        unsafe {
            let _ = SetFocus(Some(edit));
        }
    }

    pub fn omnibox_has_focus(&self) -> bool {
        self.inner().is_some_and(|i| {
            let state = i.borrow();
            state.synthetic_omnibox_focus || unsafe { GetFocus() } == state.edit
        })
    }

    /// Keep the native fixture independent of focus in the person's other desktop windows.
    pub fn set_fixture_omnibox_focus(&self, focused: bool) {
        if let Some(inner) = self.inner() { inner.borrow_mut().synthetic_omnibox_focus = focused; }
    }

    /// The text in the address bar.
    pub fn omnibox_text(&self) -> String {
        self.inner().map(|i| edit_text(i.borrow().edit)).unwrap_or_default()
    }

    /// Replace the address bar text and select `select_from..` (inline autocompletion).
    pub fn set_omnibox_text(&self, text: &str, select_from: usize) {
        let Some(inner) = self.inner() else { return };
        let mut i = inner.borrow_mut();
        set_edit_text(&mut i, text);
        let len = text.encode_utf16().count();
        let from = text.chars().take(select_from).map(char::len_utf16).sum::<usize>();
        // SAFETY: live child control; selection within the text.
        unsafe {
            SendMessageW(i.edit, EM_SETSEL, Some(WPARAM(from)), Some(LPARAM(len as isize)));
        }
    }

    /// A left click at `x, y` (DIPs in the frame), delivered as the window's own button messages
    /// (the self-test's way to click without moving the person's mouse).
    pub fn press(&self, x: f32, y: f32) {
        let scale = input::scale_of(self.hwnd);
        let at = (((y * scale) as i32 as u32 as usize) << 16) | ((x * scale) as i32 as u16 as usize);
        // SAFETY: messages to our own live window on the UI thread.
        unsafe {
            SendMessageW(self.hwnd, windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN, Some(WPARAM(1)), Some(LPARAM(at as isize)));
            SendMessageW(self.hwnd, windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP, Some(WPARAM(0)), Some(LPARAM(at as isize)));
        }
    }

    /// Send the normal mouse-move path without moving the physical pointer (native fixture).
    pub fn hover_tab(&self, id: u64) -> bool {
        let point = self.inner().and_then(|inner| {
            inner.borrow().hits.iter().find_map(|(r,hit)| (*hit == Hit::Tab(id)).then_some((r.left + 12.0,(r.top+r.bottom)/2.0)))
        });
        let Some((x,y)) = point else { return false; };
        if let Some(inner)=self.inner() {
            let mut state=inner.borrow_mut();
            state.synthetic_hover=true;
            state.preview_tab=None;
        }
        let scale = input::scale_of(self.hwnd);
        let at = (((y*scale) as i32 as u32 as usize)<<16) | ((x*scale) as i32 as u16 as usize);
        unsafe { SendMessageW(self.hwnd,windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE,Some(WPARAM(0)),Some(LPARAM(at as isize))); }
        true
    }

    /// Where the address bar is (DIPs in the frame), for placing the suggestions below it.
    pub fn omnibox_rect(&self) -> (f32, f32, f32, f32) {
        self.inner().map_or((0.0, 0.0, 0.0, 0.0), |i| {
            let r = i.borrow().omni_rect;
            (r.left, r.top, r.right - r.left, r.bottom - r.top)
        })
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: destroying our own window; WM_DESTROY frees Inner.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Screen point of a frame point (DIPs), for menus opened from the frame.
pub fn frame_to_screen(frame: isize, x: f32, y: f32) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    let hwnd = HWND(frame as _);
    let scale = scale_of(hwnd);
    let mut pt = POINT { x: (x * scale).round() as i32, y: (y * scale).round() as i32 };
    // SAFETY: `pt` is a writable local; `hwnd` is the live frame.
    unsafe {
        let _ = ClientToScreen(hwnd, &mut pt);
    }
    (pt.x, pt.y)
}
