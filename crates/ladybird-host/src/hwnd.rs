//! Child HWND surface for the Ladybird stub host (Windows).
//! Paints an explanatory page into Kakangkung's parent window until LibWeb is linked.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::Once;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, ExtTextOutW, FillRect, GetStockObject, InvalidateRect, SetBkMode, SetTextColor, SetTextAlign,
    DKGRAY_BRUSH, ETO_OPTIONS, HBRUSH, PAINTSTRUCT, TA_LEFT, TA_TOP, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, LoadCursorW, MoveWindow,
    RegisterClassW, SetWindowLongPtrW, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, IDC_ARROW, SW_SHOW,
    WINDOW_EX_STYLE, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let brush = HBRUSH(GetStockObject(DKGRAY_BRUSH).0);
            FillRect(hdc, &rc, brush);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00F1ECE7));
            SetTextAlign(hdc, TA_LEFT | TA_TOP);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let text = &*(ptr as *const Vec<u16>);
                let mut y = rc.top + 16;
                let mut start = 0usize;
                while start < text.len() {
                    let end = text[start..]
                        .iter()
                        .position(|&c| c == 0 || c == b'\n' as u16 || c == b'\r' as u16)
                        .map(|i| start + i)
                        .unwrap_or(text.len());
                    if end > start {
                        let _ = ExtTextOutW(hdc, rc.left + 16, y, ETO_OPTIONS(0), None, &text[start..end], None);
                    }
                    y += 18;
                    start = if end < text.len() && text[end] == 0 { text.len() } else { end + 1 };
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                drop(Box::from_raw(ptr as *mut Vec<u16>));
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn ensure_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).expect("module");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("KakangkungLadybirdStubHost"),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
    });
}

fn label_for(url: &str) -> Vec<u16> {
    wide(&format!(
        "Kakangkung · Ladybird stub\r\n\r\n{url}\r\n\r\nLibWeb not linked in this host.\r\nBuild the C++ UI port on Boat (HAVE_LADYBIRD=1).\r\nhttps://boat.dev/dashboard"
    ))
}

pub struct Surface {
    hwnd: HWND,
}

impl Surface {
    pub fn create(parent: usize, x: i32, y: i32, w: i32, h: i32, url: &str) -> Option<Self> {
        if parent == 0 {
            return None;
        }
        ensure_class();
        let text = Box::into_raw(Box::new(label_for(url)));
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("KakangkungLadybirdStubHost"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w.max(1),
                h.max(1),
                Some(HWND(parent as _)),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, text as isize);
            let _ = ShowWindow(hwnd, SW_SHOW);
            Some(Self { hwnd })
        }
    }

    pub fn set_bounds(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            let _ = MoveWindow(self.hwnd, x, y, w.max(1), h.max(1), true);
        }
    }

    pub fn set_label(&mut self, url: &str) {
        unsafe {
            let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let text = &mut *(ptr as *mut Vec<u16>);
                *text = label_for(url);
                let _ = InvalidateRect(Some(self.hwnd), None, true);
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
