//! Native built-in pages (Settings, History, …) drawn in the page area with Direct2D, laid out
//! like Chrome's: a header with a search box, navigation on the left and cards of rows.
//!
//! The browser describes a page (`PageDoc`); this window draws it, scrolls it smoothly, filters
//! it with the search box and reports what the person does as browser commands:
//! `page\t<toggle|select|radio|button|text|link|nav>\t<id>\t<value>`, plus `key\t…` for browser
//! shortcuts typed while the page has the keyboard.

mod art;
mod input;
mod layout;
pub mod model;
mod paint;
pub mod start;

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::Instant;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush, D2D1_FACTORY_TYPE_SINGLE_THREADED};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowLongPtrW, IsWindowVisible, LoadCursorW, MoveWindow, RegisterClassExW, SetTimer, SetWindowTextW, ShowWindow,
    ES_AUTOHSCROLL, GWLP_USERDATA, IDC_ARROW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
};

use self::layout::Layout;
pub use self::model::{Bounce, Button, ButtonStyle, Control, NavItem, PageDoc, Row, Section};
use crate::frame::paint::{icon_family, text_format};

/// Timer ids of the page window.
const TIMER_SCROLL: usize = 1;
const TIMER_TOAST: usize = 2;
const TIMER_ANIM: usize = 3;
/// Animation lengths at normal speed (ms): the page coming in, a tile's own rise, a switch, a
/// hover highlight fading in.
const ENTER_MS: f32 = 280.0;
const TILE_MS: f32 = 340.0;
const TILE_DELAY_MS: f32 = 70.0;
const TILE_STAGGER_MS: f32 = 35.0;
const TOGGLE_MS: f32 = 200.0;
const HOVER_MS: f32 = 140.0;
/// The logo dropped in (once), and one hop of the waiting logo (on and on).
const BOUNCE_MS: f32 = 1100.0;
const HOP_MS: f32 = 560.0;
const TOAST_MS: u32 = 3000;

struct Fonts {
    header: IDWriteTextFormat,
    section: IDWriteTextFormat,
    title: IDWriteTextFormat,
    sub: IDWriteTextFormat,
    heading: IDWriteTextFormat,
    nav: IDWriteTextFormat,
    nav_on: IDWriteTextFormat,
    button: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    icons_small: IDWriteTextFormat,
    hero: IDWriteTextFormat,
    hero_text: IDWriteTextFormat,
    hero_icon: IDWriteTextFormat,
    logo: IDWriteTextFormat,
    tile: IDWriteTextFormat,
    art: IDWriteTextFormat,
    sparkle: IDWriteTextFormat,
    title_center: IDWriteTextFormat,
    logo_big: IDWriteTextFormat,
}

struct Inner {
    hwnd: HWND,
    /// Search box in the header.
    search: HWND,
    /// Text fields of the rows, by control id.
    edits: Vec<(String, HWND)>,
    /// Background brush of the text fields: (colour, brush).
    edit_brush: (u32, HBRUSH),
    edit_font: (f32, HFONT),
    doc: PageDoc,
    dark: bool,
    filter: String,
    /// Section on show (each section is a page of its own, like Chrome's settings).
    section: String,
    layout: Layout,
    scroll: f32,
    /// Where smooth scrolling is heading.
    target: f32,
    hover: Option<input::Target>,
    /// When the page (or section) came in, when the hover moved, and switches still sliding.
    enter: Instant,
    /// The page is coming in but no frame of it has been drawn yet.
    enter_pending: bool,
    hover_at: Instant,
    toggles: Vec<(String, Instant)>,
    /// The pointer is over the window (the scrollbar shows), over the scrollbar (it widens).
    mouse_in: bool,
    bar_hover: bool,
    /// Scrollbar drag: (pointer y, scroll) at the press.
    bar_drag: Option<(f32, f32)>,
    toast: String,
    factory: ID2D1Factory,
    rt: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    dw: IDWriteFactory,
    fonts: Fonts,
    on_command: Box<dyn FnMut(String)>,
}

/// The page window (hidden until a native page is shown). Dropping it destroys the window.
pub struct Page {
    hwnd: HWND,
}

fn fonts(dw: &IDWriteFactory) -> windows::core::Result<Fonts> {
    let ui = w!("Segoe UI");
    let (n, b, lead, center) = (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_CENTER);
    let sub = text_format(dw, ui, 12.5, n, lead)?;
    let hero_text = text_format(dw, ui, 14.0, n, center)?;
    // SAFETY: plain setters on formats we own.
    unsafe {
        for f in [&sub, &hero_text] {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
        }
    }
    Ok(Fonts {
        header: text_format(dw, ui, 20.0, b, lead)?,
        section: text_format(dw, ui, 16.0, b, lead)?,
        title: text_format(dw, ui, 14.0, n, lead)?,
        sub,
        heading: text_format(dw, ui, 11.5, b, lead)?,
        nav: text_format(dw, ui, 13.5, n, lead)?,
        nav_on: text_format(dw, ui, 13.5, b, lead)?,
        button: text_format(dw, ui, 13.0, b, center)?,
        icons: text_format(dw, icon_family(dw), 16.0, n, center)?,
        icons_small: text_format(dw, icon_family(dw), 11.0, n, center)?,
        hero: text_format(dw, ui, 28.0, n, center)?,
        hero_text,
        hero_icon: text_format(dw, icon_family(dw), 34.0, n, center)?,
        logo: text_format(dw, ui, 32.0, b, center)?,
        tile: text_format(dw, ui, 12.5, n, center)?,
        art: text_format(dw, icon_family(dw), 34.0, n, center)?,
        sparkle: text_format(dw, w!("Segoe UI Symbol"), 14.0, n, center)?,
        title_center: text_format(dw, ui, 16.0, b, center)?,
        logo_big: text_format(dw, ui, 56.0, b, center)?,
    })
}

impl Page {
    /// Create the (hidden) page window as a child of the browser window `parent`.
    /// `on_command` receives the commands on the UI thread.
    pub fn create(parent: isize, on_command: impl FnMut(String) + 'static) -> windows::core::Result<Page> {
        // SAFETY: window-class registration and child-window creation on the UI thread; the boxed
        // Inner belongs to the window (GWLP_USERDATA) until WM_DESTROY frees it.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(input::page_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: w!("KakangkungPage"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let inner = Box::new(RefCell::new(Inner {
                hwnd: HWND::default(),
                search: HWND::default(),
                edits: vec![],
                edit_brush: (u32::MAX, HBRUSH::default()),
                edit_font: (0.0, HFONT::default()),
                doc: PageDoc::default(),
                dark: false,
                filter: String::new(),
                section: String::new(),
                layout: Layout::default(),
                scroll: 0.0,
                target: 0.0,
                hover: None,
                enter: crate::motion::long_ago(),
                enter_pending: false,
                hover_at: crate::motion::long_ago(),
                toggles: vec![],
                mouse_in: false,
                bar_hover: false,
                bar_drag: None,
                toast: String::new(),
                factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                rt: None,
                brush: None,
                fonts: fonts(&dw)?,
                dw,
                on_command: Box::new(on_command),
            }));
            let raw = Box::into_raw(inner);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("KakangkungPage"),
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN,
                0,
                0,
                100,
                100,
                Some(HWND(parent as _)),
                None,
                Some(instance.into()),
                Some(raw as *const c_void),
            )?;
            let search = new_edit(hwnd)?;
            (*raw).borrow_mut().search = search;
            Ok(Page { hwnd })
        }
    }

    fn inner(&self) -> Option<&RefCell<Inner>> {
        // SAFETY: GWLP_USERDATA holds the boxed Inner from creation until WM_DESTROY.
        unsafe { (GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>).as_ref() }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Position in the parent's client area, physical pixels.
    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        // SAFETY: our own live window.
        unsafe {
            let _ = MoveWindow(self.hwnd, x, y, width, height, true);
        }
    }

    /// Show or hide the page; showing gives it the keyboard. Hidden, its drawing surface is
    /// let go (made again on the next paint).
    pub fn set_visible(&self, visible: bool) {
        if !visible {
            if let Some(cell) = self.inner() {
                let mut i = cell.borrow_mut();
                i.rt = None;
                i.brush = None;
                // SAFETY: our own window's frame timer.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(i.hwnd), TIMER_ANIM);
                }
            }
        }
        // SAFETY: our own live window.
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
            if visible {
                let _ = SetFocus(Some(self.hwnd));
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: plain query on our own window.
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    /// Draw `doc`. A different page starts at the top with an empty search box;
    /// the same page keeps its scroll position and search (a refresh after a change).
    pub fn set_doc(&self, doc: PageDoc, dark: bool) {
        let Some(cell) = self.inner() else { return };
        let mut i = cell.borrow_mut();
        // Another page (not a refresh of this one): pages without a title (the start pages) are
        // told apart by their kind and search box.
        let fresh = i.doc.title != doc.title || i.doc.start != doc.start || i.doc.fakebox != doc.fakebox;
        if fresh {
            // A message belongs to the page it was shown on.
            i.toast.clear();
        }
        if !doc.toast.is_empty() {
            i.toast = doc.toast.clone();
            // SAFETY: timer on our own window; it clears the message.
            unsafe {
                SetTimer(Some(i.hwnd), TIMER_TOAST, TOAST_MS, None);
            }
        }
        i.dark = dark;
        i.doc = doc;
        if fresh {
            i.start_enter();
            i.section = if i.doc.paged { i.doc.sections.first().map(|s| s.id.clone()).unwrap_or_default() } else { String::new() };
            i.scroll = 0.0;
            i.target = 0.0;
            i.filter.clear();
            let search = i.search;
            drop(i);
            // SAFETY: our own child control (EN_CHANGE re-borrows Inner, hence the drop above).
            unsafe {
                let _ = SetWindowTextW(search, w!(""));
            }
            i = cell.borrow_mut();
        }
        i.sync_edits();
        i.relayout();
    }

    /// Show section `id` (an address such as kakangkung://settings/privacy): its own page on a
    /// paged page, else scrolled to.
    pub fn show_section(&self, id: &str) {
        let Some(cell) = self.inner() else { return };
        let mut i = cell.borrow_mut();
        if i.doc.paged && i.doc.sections.iter().any(|s| s.id == id) {
            i.open_section(id);
        } else if let Some(y) = i.layout.anchors.iter().find(|(a, _)| a == id).map(|(_, y)| *y) {
            i.scroll_smooth(y);
        }
    }

    /// The section on show.
    pub fn section(&self) -> String {
        self.inner().map(|c| c.borrow().section.clone()).unwrap_or_default()
    }

    /// The page on show (for the self-test).
    pub fn doc(&self) -> PageDoc {
        self.inner().map(|c| c.borrow().doc.clone()).unwrap_or_default()
    }

    /// Type into the search box (self-test) and return the ids of the sections left.
    pub fn search(&self, text: &str) -> Vec<String> {
        let Some(cell) = self.inner() else { return vec![] };
        let search = cell.borrow().search;
        // SAFETY: our own child control; EN_CHANGE runs the filter.
        unsafe {
            let _ = SetWindowTextW(search, &HSTRING::from(text));
        }
        cell.borrow().layout.anchors.iter().map(|(id, _)| id.clone()).collect()
    }

    /// Does the page (or one of its fields) have the keyboard?
    pub fn has_focus(&self) -> bool {
        // SAFETY: plain focus query.
        let f = unsafe { GetFocus() };
        self.inner().is_some_and(|c| {
            let i = c.borrow();
            f == i.hwnd || f == i.search || i.edits.iter().any(|(_, e)| *e == f)
        })
    }
}

/// Id of the start page's search box among the page's text fields.
pub(super) const FAKEBOX: &str = "fakebox";

impl Page {
    /// The start page's search box (the New Tab page's): its text.
    pub fn fakebox_text(&self) -> String {
        self.inner().and_then(|c| c.borrow().fakebox_edit()).map(crate::frame::edit_text).unwrap_or_default()
    }

    /// Put `text` in the start page's search box, selected from character `select_from`.
    pub fn set_fakebox_text(&self, text: &str, select_from: usize) {
        let Some(cell) = self.inner() else { return };
        // Held during the change: the page's EN_CHANGE then finds Inner borrowed and reports
        // nothing, so text the browser puts in (completion, ↑ ↓) is not taken for typing.
        let i = cell.borrow();
        let Some(edit) = i.fakebox_edit() else { return };
        let from = text.chars().take(select_from).map(char::len_utf16).sum::<usize>();
        // SAFETY: plain calls on our own child control.
        unsafe {
            let _ = SetWindowTextW(edit, &HSTRING::from(text));
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(edit, windows::Win32::UI::Controls::EM_SETSEL, Some(windows::Win32::Foundation::WPARAM(from)), Some(windows::Win32::Foundation::LPARAM(text.encode_utf16().count() as isize)));
        }
    }

    /// Does the start page's search box have the keyboard?
    pub fn fakebox_has_focus(&self) -> bool {
        // SAFETY: plain query.
        let focus = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
        self.inner().and_then(|c| c.borrow().fakebox_edit()).is_some_and(|e| e == focus)
    }

    /// The start page's search box: x, y, width, height in DIPs of the page window.
    pub fn fakebox_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let c = self.inner()?;
        let i = c.borrow();
        let r = i.layout.fakebox?;
        let top = r.top + layout::HEADER_H - i.scroll;
        Some((r.left, top, r.right - r.left, r.bottom - r.top))
    }
}

/// A borderless EDIT inside the page (search box or a row's text field).
fn new_edit(parent: HWND) -> windows::core::Result<HWND> {
    // SAFETY: child control creation on the UI thread; the subclass data is the parent's Inner,
    // alive as long as the parent (and so the child) is.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            0,
            0,
            10,
            10,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )?;
        let data = GetWindowLongPtrW(parent, GWLP_USERDATA) as usize;
        let _ = SetWindowSubclass(edit, Some(input::edit_proc), 1, data);
        Ok(edit)
    }
}

impl Inner {
    /// The start page's search box field, if the page has one.
    pub(super) fn fakebox_edit(&self) -> Option<HWND> {
        self.edits.iter().find(|(id, _)| id == FAKEBOX).map(|(_, e)| *e)
    }

    /// One native text field per `Control::Text` of the page; values follow the description
    /// unless the person is typing in that field.
    fn sync_edits(&mut self) {
        let mut wanted: Vec<(String, String)> = self
            .doc
            .sections
            .iter()
            .flat_map(|s| s.cards.iter().flatten())
            .filter_map(|r| match &r.control {
                Control::Text { id, value, .. } => Some((id.clone(), value.clone())),
                _ => None,
            })
            .collect();
        // The start page's search box is a real field too (its text is the person's alone).
        if self.doc.start && !self.doc.fakebox.is_empty() {
            wanted.push((FAKEBOX.into(), String::new()));
        }
        // SAFETY: plain calls on our own child controls.
        unsafe {
            for (id, edit) in std::mem::take(&mut self.edits) {
                if wanted.iter().any(|(w, _)| *w == id) {
                    self.edits.push((id, edit));
                } else {
                    let _ = DestroyWindow(edit);
                }
            }
            let focus = GetFocus();
            for (id, value) in wanted {
                let edit = match self.edits.iter().find(|(e, _)| *e == id) {
                    Some((_, e)) => *e,
                    None => match new_edit(self.hwnd) {
                        Ok(e) => {
                            self.edits.push((id.clone(), e));
                            e
                        }
                        Err(_) => continue,
                    },
                };
                if edit != focus && id != FAKEBOX {
                    let _ = SetWindowTextW(edit, &HSTRING::from(value.replace('\n', "  ")));
                }
            }
        }
    }

    fn repaint(&self) {
        // SAFETY: repaint request for our own window.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // SAFETY: destroying our own window; WM_DESTROY frees Inner.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
