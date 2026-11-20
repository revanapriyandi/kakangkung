//! Child HWND surface for the Ladybird stub host (Windows).
//! Paints an explanatory page into Kakangkung's parent window until LibWeb is linked.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::Once;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, FillRect, GetDC,
    GetDIBits, GetStockObject, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextAlign, SetTextColor,
    TextOutW, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DKGRAY_BRUSH, HBRUSH, PAINTSTRUCT, TA_LEFT, TA_TOP,
    TRANSPARENT,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS, PW_CLIENTONLY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, LoadCursorW, MoveWindow,
    RegisterClassW, SetWindowLongPtrW, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, IDC_ARROW, PW_RENDERFULLCONTENT,
    SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
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
                        let _ = TextOutW(hdc, rc.left + 16, y, &text[start..end]);
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

    pub fn set_visible(&mut self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }

    /// Capture the stub surface client area to a PNG (same path the full host will use for LibWeb).
    pub fn capture_png(&self, path: &str) -> Result<(), String> {
        unsafe {
            let mut rc = RECT::default();
            GetClientRect(self.hwnd, &mut rc).map_err(|e| e.to_string())?;
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            if w <= 0 || h <= 0 {
                return Err("empty surface".into());
            }
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            let screen = GetDC(Some(self.hwnd));
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            let _ = PrintWindow(self.hwnd, mem, PRINT_WINDOW_FLAGS(PW_CLIENTONLY.0 | PW_RENDERFULLCONTENT));
            SelectObject(mem, old);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let n = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(Some(self.hwnd), screen);
            if n != h {
                return Err(format!("GetDIBits {n}/{h}"));
            }
            let rgb: Vec<u8> = bgra.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0]]).collect();
            std::fs::write(path, encode_png(w as u32, h as u32, &rgb)).map_err(|e| e.to_string())
        }
    }
}

fn encode_png(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity(rgb.len() + h as usize);
    for row in rgb.chunks_exact(w as usize * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
