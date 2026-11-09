//! Native DevTools hosting workaround. WebView2 has no supported docking API.
//! Only a newly created DevTools window in this WebView's browser process is adopted.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::{Duration, Instant}};
use windows::{core::{BOOL, w}, Win32::{Foundation::{HWND, LPARAM, WPARAM}, UI::{
    Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
    HiDpi::{AreDpiAwarenessContextsEqual, GetWindowDpiAwarenessContext, GetDpiForWindow},
    WindowsAndMessaging::*,
}}};
use wry::{WebView, WebViewExtWindows};

/// Replace only WebView2's native Inspect command with the browser's docked route.
/// The caller should enqueue its action; native context-menu callbacks must return promptly.
pub fn watch_context_menu(webview: &WebView, on_inspect: impl Fn() + 'static) -> Option<i64> {
    use webview2_com::{take_pwstr, ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_11, ICoreWebView2Environment9, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
    };
    use windows::core::{Interface, HSTRING, PWSTR};
    let core = webview.webview().cast::<ICoreWebView2_11>().ok()?;
    let environment = webview.environment().cast::<ICoreWebView2Environment9>().ok()?;
    let inspect = unsafe {
        environment.CreateContextMenuItem(&HSTRING::from("Inspect"), None, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)
    }.ok()?;
    let selected = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
        on_inspect();
        Ok(())
    }));
    let mut selected_token = 0;
    unsafe { inspect.add_CustomItemSelected(&selected, &mut selected_token) }.ok()?;
    // Keep one custom item per WebView, as recommended by WebView2. Its lifetime and
    // selection handler are owned by this request registration, avoiding per-menu leaks.
    let requested = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            let items = args.MenuItems()?;
            let mut count = 0;
            items.Count(&mut count)?;
            for index in 0..count {
                let item = items.GetValueAtIndex(index)?;
                let mut name = PWSTR::null();
                item.Name(&mut name)?;
                if take_pwstr(name) == "inspectElement" {
                    items.RemoveValueAtIndex(index)?;
                    items.InsertValueAtIndex(index, &inspect)?;
                    break;
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_ContextMenuRequested(&requested, &mut token) }.ok()?;
    Some(token)
}

pub fn unwatch_context_menu(webview: &WebView, token: i64) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_11;
    use windows::core::Interface;
    if let Ok(core) = webview.webview().cast::<ICoreWebView2_11>() {
        unsafe { let _ = core.remove_ContextMenuRequested(token); }
    }
}

thread_local! {
    static CAPTURES: RefCell<HashMap<isize, Rc<RefCell<Capture>>>> = RefCell::new(HashMap::new());
    static CANCEL_TIMERS: RefCell<HashMap<usize, isize>> = RefCell::new(HashMap::new());
}

struct Capture {
    pid: u32,
    baseline: Vec<isize>,
    found: Vec<isize>,
    cancelled: bool,
}

/// UI-thread-only owner of one real WebView2 DevTools window.
pub struct DockedDevTools {
    parent: HWND,
    capture: Rc<RefCell<Capture>>,
    hook: HWINEVENTHOOK,
    child: Option<HWND>,
    viewport: Option<HWND>,
    caption_height: i32,
    caption_dpi: u32,
    opened: Instant,
    closed: bool,
}

impl DockedDevTools {
    pub fn open(webview: &WebView, parent: isize) -> Result<Self, String> {
        let parent = HWND(parent as *mut _);
        if !unsafe { IsWindow(Some(parent)) }.as_bool() {
            return Err("DevTools host window is unavailable".into());
        }
        let mut pid = 0;
        unsafe { webview.webview().BrowserProcessId(&mut pid) }.map_err(|e| e.to_string())?;
        if pid == 0 { return Err("WebView browser process is unavailable".into()); }
        if CAPTURES.with(|all| all.borrow().values().any(|c| c.borrow().pid == pid)) {
            return Err("DevTools is still opening or closing; please try again shortly".into());
        }
        let baseline = windows_for_process(pid);
        // OpenDevToolsWindow reuses an existing inspector. Never adopt a pre-existing window.
        if baseline.iter().any(|h| is_devtools(HWND(*h as *mut _), pid)) {
            return Err("Close the existing DevTools window before opening the docked inspector".into());
        }
        let capture = Rc::new(RefCell::new(Capture { pid, baseline, found: Vec::new(), cancelled: false }));
        // Out-of-context events arrive on this UI thread while its normal message loop runs.
        let hook = unsafe {
            SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_NAMECHANGE, None, Some(on_window_event), pid, 0, WINEVENT_OUTOFCONTEXT)
        };
        if hook.0.is_null() { return Err("Cannot monitor the DevTools window safely".into()); }
        CAPTURES.with(|all| { all.borrow_mut().insert(hook.0 as isize, capture.clone()); });
        let owner = Self { parent, capture, hook, child: None, viewport: None, caption_height: 0, caption_dpi: 96, opened: Instant::now(), closed: false };
        unsafe { webview.webview().OpenDevToolsWindow() }.map_err(|e| e.to_string())?;
        Ok(owner)
    }

    /// Called from the normal UI tick. `false` means creation is still pending.
    pub fn poll(&mut self, bounds: [i32; 4]) -> Result<bool, String> {
        if self.closed { return Err("DevTools has closed".into()); }
        if let Some(child) = self.child {
            let pid = self.capture.borrow().pid;
            for raw in windows_for_process(pid) { capture_window(&self.capture, HWND(raw as *mut _)); }
            if self.capture.borrow().found.iter().any(|raw| *raw != child.0 as isize) {
                self.close();
                return Err("DevTools changed its native window; reopen the docked inspector".into());
            }
            if !self.owns(child) { self.close(); return Err("DevTools has closed".into()); }
            self.resize(bounds)?;
            return Ok(true);
        }
        // Also inspect hidden windows: a SHOW event can precede the final title assignment.
        let pid = self.capture.borrow().pid;
        for raw in windows_for_process(pid) { capture_window(&self.capture, HWND(raw as *mut _)); }
        let found = self.capture.borrow().found.clone();
        if found.len() > 1 { self.close(); return Err("DevTools window identity is ambiguous".into()); }
        if let Some(raw) = found.first() {
            let child = HWND(*raw as *mut _);
            if !is_devtools(child, pid) { self.close(); return Err("DevTools window is unavailable".into()); }
            let same_dpi = unsafe {
                AreDpiAwarenessContextsEqual(GetWindowDpiAwarenessContext(self.parent), GetWindowDpiAwarenessContext(child))
            }.as_bool();
            if !same_dpi { self.close(); return Err("DevTools cannot dock with a different Windows DPI mode".into()); }
            // Chromium paints its own caption inside the client surface even after WS_CAPTION
            // is removed. Clip that surface in a native child viewport, keeping tool tabs intact.
            self.caption_height = chromium_caption_height(child);
            self.caption_dpi = unsafe { GetDpiForWindow(child) }.max(96);
            let viewport = unsafe {
                CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""),
                    WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                    bounds[0], bounds[1], bounds[2].max(1), bounds[3].max(1),
                    Some(self.parent), None, None, None)
            }.map_err(|e| format!("Cannot create DevTools viewport: {e}"))?;
            self.viewport = Some(viewport);
            // SetParent does not change styles. Hide first, remove desktop chrome, then adopt.
            unsafe {
                let _ = ShowWindow(child, SW_HIDE);
                let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
                let style = (style & !(WS_POPUP.0 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0))
                    | WS_CHILD.0 | WS_CLIPSIBLINGS.0;
                SetWindowLongPtrW(child, GWL_STYLE, style as isize);
                let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
                SetWindowLongPtrW(child, GWL_EXSTYLE, (ex & !WS_EX_APPWINDOW.0) as isize);
                // A null previous parent can also be a successful result; verify the relationship.
                let _ = SetParent(child, Some(viewport));
                if GetParent(child).ok() != Some(viewport) {
                    self.close(); return Err("Windows refused to dock DevTools".into());
                }
            }
            self.child = Some(child);
            if let Err(e) = self.resize(bounds) { self.close(); return Err(e); }
            unsafe {
                let _ = ShowWindow(viewport, SW_SHOWNA);
                let _ = ShowWindow(child, SW_SHOWNA);
            }
            return Ok(true);
        }
        if self.opened.elapsed() >= Duration::from_secs(8) {
            self.close();
            return Err("DevTools did not create an identifiable window".into());
        }
        Ok(false)
    }

    /// Bounds are physical client pixels: x, y, width, height.
    pub fn resize(&self, [x, y, width, height]: [i32; 4]) -> Result<(), String> {
        if let Some(child) = self.child {
            if !self.owns(child) { return Err("DevTools has closed".into()); }
            let Some(viewport) = self.viewport else { return Err("DevTools viewport is unavailable".into()); };
            let caption = self.caption_offset(child);
            unsafe {
                SetWindowPos(viewport, Some(HWND_TOP), x, y, width.max(1), height.max(1), SWP_NOACTIVATE)
                    .map_err(|e| e.to_string())?;
                SetWindowPos(child, Some(HWND_TOP), 0, -caption, width.max(1), height.max(1).saturating_add(caption), SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_ASYNCWINDOWPOS)
            }.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn close(&mut self) {
        if self.closed { return; }
        self.closed = true;
        // Cancellation can race asynchronous creation. Retain the narrowly scoped watcher
        // briefly so a late inspector is closed rather than left on the desktop.
        self.capture.borrow_mut().cancelled = true;
        let pid = self.capture.borrow().pid;
        for raw in windows_for_process(pid) { capture_window(&self.capture, HWND(raw as *mut _)); }
        if self.child.is_none() && !self.hook.0.is_null() {
            let timer = unsafe { SetTimer(None, 0, 8_000, Some(expire_cancelled)) };
            if timer != 0 {
                CANCEL_TIMERS.with(|timers| { timers.borrow_mut().insert(timer, self.hook.0 as isize); });
                self.hook = HWINEVENTHOOK::default();
            }
        }
        self.unhook();
        let pid = self.capture.borrow().pid;
        let found = self.capture.borrow().found.clone();
        for raw in found {
            let window = HWND(raw as *mut _);
            if is_devtools(window, pid) {
                unsafe {
                    let _ = ShowWindow(window, SW_HIDE);
                    // Foreign-process HWNDs must be closed by their owning thread.
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
        self.child = None;
        if let Some(viewport) = self.viewport.take() {
            // Detach the foreign window before destroying our host; its owner handles WM_CLOSE.
            for raw in self.capture.borrow().found.iter() {
                let window = HWND(*raw as *mut _);
                if unsafe { GetParent(window) }.ok() == Some(viewport) {
                    unsafe { let _ = SetParent(window, None); }
                }
            }
            unsafe { let _ = DestroyWindow(viewport); }
        }
    }

    fn owns(&self, child: HWND) -> bool {
        let Some(viewport) = self.viewport else { return false };
        is_devtools(child, self.capture.borrow().pid)
            && unsafe { GetParent(child) }.ok() == Some(viewport)
    }

    pub fn caption_hidden(&self) -> bool {
        let (Some(child), Some(viewport)) = (self.child, self.viewport) else { return false };
        let mut child_rect = windows::Win32::Foundation::RECT::default();
        let mut host_rect = windows::Win32::Foundation::RECT::default();
        self.owns(child) && self.caption_height > 0 && unsafe {
            GetWindowRect(child, &mut child_rect).is_ok()
                && GetWindowRect(viewport, &mut host_rect).is_ok()
                && child_rect.top + self.caption_offset(child) == host_rect.top
                && (GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_CAPTION.0) == 0
        }
    }

    fn caption_offset(&self, child: HWND) -> i32 {
        let dpi = unsafe { GetDpiForWindow(child) }.max(96);
        (self.caption_height as i64 * dpi as i64 / self.caption_dpi as i64) as i32
    }

    fn unhook(&mut self) {
        if !self.hook.0.is_null() {
            CAPTURES.with(|all| { all.borrow_mut().remove(&(self.hook.0 as isize)); });
            unsafe { let _ = UnhookWinEvent(self.hook); }
            self.hook = HWINEVENTHOOK::default();
        }
    }
}

fn chromium_caption_height(window: HWND) -> i32 {
    let mut info = TITLEBARINFO { cbSize: std::mem::size_of::<TITLEBARINFO>() as u32, ..Default::default() };
    let dpi = unsafe { GetDpiForWindow(window) }.max(96);
    if unsafe { GetTitleBarInfo(window, &mut info) }.is_ok() {
        let height = info.rcTitleBar.bottom - info.rcTitleBar.top;
        if height > 0 && height <= (80 * dpi / 96) as i32 { return height; }
    }
    (28 * dpi / 96) as i32
}

impl Drop for DockedDevTools { fn drop(&mut self) { self.close(); } }

unsafe extern "system" fn on_window_event(hook: HWINEVENTHOOK, _event: u32, hwnd: HWND, object: i32, child: i32, _thread: u32, _time: u32) {
    if object != OBJID_WINDOW.0 || child != 0 { return; }
    let capture = CAPTURES.with(|all| all.borrow().get(&(hook.0 as isize)).cloned());
    if let Some(capture) = capture { capture_window(&capture, hwnd); }
}

fn capture_window(capture: &Rc<RefCell<Capture>>, hwnd: HWND) {
    let raw = hwnd.0 as isize;
    let eligible = {
        let c = capture.borrow();
        !c.baseline.contains(&raw)
            && unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd
            && is_devtools(hwnd, c.pid)
    };
    if !eligible { return; }
    {
        let mut c = capture.borrow_mut();
        if !c.found.contains(&raw) { c.found.push(raw); }
    }
    unsafe { let _ = ShowWindow(hwnd, SW_HIDE); }
    if capture.borrow().cancelled {
        unsafe { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }
}

unsafe extern "system" fn expire_cancelled(_hwnd: HWND, _message: u32, timer: usize, _time: u32) {
    unsafe { let _ = KillTimer(None, timer); }
    let hook = CANCEL_TIMERS.with(|timers| timers.borrow_mut().remove(&timer));
    if let Some(raw) = hook {
        CAPTURES.with(|all| { all.borrow_mut().remove(&raw); });
        unsafe { let _ = UnhookWinEvent(HWINEVENTHOOK(raw as *mut _)); }
    }
}

fn is_devtools(hwnd: HWND, pid: u32) -> bool {
    let mut actual = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual)); }
    if actual != pid || pid == 0 { return false; }
    let mut class = [0u16; 128];
    let mut title = [0u16; 1024];
    let (class_len, title_len) = unsafe { (GetClassNameW(hwnd, &mut class), GetWindowTextW(hwnd, &mut title)) };
    let class = String::from_utf16_lossy(&class[..class_len.max(0) as usize]);
    let title = String::from_utf16_lossy(&title[..title_len.max(0) as usize]);
    devtools_marker(&class, &title)
}

fn devtools_marker(class: &str, title: &str) -> bool {
    class.starts_with("Chrome_WidgetWin_")
        && (title == "DevTools" || title.starts_with("DevTools - ") || title.starts_with("Developer Tools - "))
}

fn windows_for_process(pid: u32) -> Vec<isize> {
    struct Search { pid: u32, handles: Vec<isize> }
    unsafe extern "system" fn collect(hwnd: HWND, context: LPARAM) -> BOOL {
        let search = unsafe { &mut *(context.0 as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
        if pid == search.pid { search.handles.push(hwnd.0 as isize); }
        BOOL(1)
    }
    let mut search = Search { pid, handles: Vec::new() };
    unsafe { let _ = EnumWindows(Some(collect), LPARAM((&mut search as *mut Search) as isize)); }
    search.handles
}

#[cfg(test)]
mod tests {
    use super::devtools_marker;
    #[test]
    fn only_native_inspector_titles_match() {
        assert!(devtools_marker("Chrome_WidgetWin_1", "DevTools - https://example.test"));
        assert!(devtools_marker("Chrome_WidgetWin_1", "DevTools"));
        assert!(!devtools_marker("Chrome_WidgetWin_1", "My DevTools tutorial"));
        assert!(!devtools_marker("Notepad", "DevTools - https://example.test"));
        assert!(!devtools_marker("Chrome_WidgetWin_1", "https://example.test"));
    }
}
