//! A single, nonactivating native tab hover card. It owns no WebView.
use crate::{
    frame::{
        layout::rect,
        paint::{icon_family, text_format},
        scale_of,
        style::{color, palette},
    },
    tab_data::Thumbnail,
};
use std::{cell::RefCell, ffi::c_void, rc::Rc};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::{
                BeginPaint, ClientToScreen, EndPaint, GetMonitorInfoW, InvalidateRect,
                MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, PAINTSTRUCT,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

#[derive(Clone, Default)]
pub struct TabPreviewData {
    pub title: String,
    pub site: String,
    pub memory: String,
    pub note: String,
    pub status: String,
    pub thumbnail: Option<Rc<Thumbnail>>,
}

const CARD_WIDTH: f32 = 320.0;
const HEADER_HEIGHT: f32 = 84.0;
const IMAGE_HEIGHT: f32 = 180.0;
const FOOTER_HEIGHT: f32 = 44.0;
const NOTE_HEIGHT: f32 = 18.0;

fn card_height(note: &str) -> f32 {
    HEADER_HEIGHT + IMAGE_HEIGHT + FOOTER_HEIGHT + if note.is_empty() { 0.0 } else { NOTE_HEIGHT }
}
struct Inner {
    hwnd: HWND,
    data: TabPreviewData,
    dark: bool,
    factory: ID2D1Factory,
    target: Option<ID2D1HwndRenderTarget>,
    bitmap: Option<ID2D1Bitmap>,
    bold: IDWriteTextFormat,
    small: IDWriteTextFormat,
    icon: IDWriteTextFormat,
}
pub struct TabPreview {
    hwnd: HWND,
    owner: HWND,
}
impl TabPreview {
    pub fn new(parent: isize) -> crate::Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            RegisterClassExW(&WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: w!("KakangkungTabPreview"),
                ..Default::default()
            });
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let bold = text_format(
                &dw,
                w!("Segoe UI"),
                14.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?;
            bold.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            bold.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            let ellipsis = dw.CreateEllipsisTrimmingSign(&bold)?;
            bold.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() }, &ellipsis)?;
            let raw = Box::into_raw(Box::new(RefCell::new(Inner {
                hwnd: HWND::default(),
                data: TabPreviewData::default(),
                dark: false,
                factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                target: None,
                bitmap: None,
                bold,
                small: text_format(
                    &dw,
                    w!("Segoe UI"),
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                )?,
                icon: text_format(&dw, icon_family(&dw), 16.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            })));
            let hwnd = match CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                w!("KakangkungTabPreview"),
                w!(""),
                WS_POPUP,
                0,
                0,
                320,
                280,
                Some(HWND(parent as _)),
                None,
                Some(instance.into()),
                Some(raw.cast::<c_void>()),
            ) {
                Ok(h) => h,
                Err(e) => {
                    drop(Box::from_raw(raw));
                    return Err(e);
                }
            };
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as _,
                std::mem::size_of_val(&corner) as u32,
            );
            Ok(Self {
                hwnd,
                owner: HWND(parent as _),
            })
        }
    }
    pub fn show(&self, data: TabPreviewData, x: f32, bottom: f32, dark: bool) -> crate::Result<()> {
        unsafe {
            let scale = scale_of(self.owner);
            let mut point = POINT {
                x: (x * scale).round() as i32,
                y: ((bottom + 4.0) * scale).round() as i32,
            };
            let _ = ClientToScreen(self.owner, &mut point);
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(
                MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            );
            let (width, height) = (
                (CARD_WIDTH * scale).round() as i32,
                (card_height(&data.note) * scale).round() as i32,
            );
            point.x = point
                .x
                .min(monitor.rcWork.right - width)
                .max(monitor.rcWork.left);
            point.y = point
                .y
                .min(monitor.rcWork.bottom - height)
                .max(monitor.rcWork.top);
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOP),
                point.x,
                point.y,
                width,
                height,
                SWP_NOACTIVATE,
            )?;
            let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>;
            if let Some(cell) = ptr.as_ref() {
                let mut i = cell.borrow_mut();
                i.data = data;
                i.dark = dark;
                i.bitmap = None;
                i.paint()?;
            }
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            Ok(())
        }
    }
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>;
            if let Some(cell) = ptr.as_ref() {
                if let Ok(mut i) = cell.try_borrow_mut() {
                    i.data = TabPreviewData::default();
                    i.bitmap = None;
                    i.target = None;
                }
            }
        }
    }
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }
    pub fn capture_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        crate::capture_window_png(self.hwnd.0 as isize,path)
    }
}
impl Drop for TabPreview {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
impl Inner {
    fn paint(&mut self) -> crate::Result<()> {
        unsafe {
            let scale = scale_of(self.hwnd);
            let mut rc = RECT::default();
            GetClientRect(self.hwnd, &mut rc)?;
            let size = D2D_SIZE_U {
                width: (rc.right - rc.left).max(1) as u32,
                height: (rc.bottom - rc.top).max(1) as u32,
            };
            if self.target.is_none() {
                self.target = Some(self.factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                        dpiX: 96.0 * scale,
                        dpiY: 96.0 * scale,
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    },
                )?);
                self.bitmap = None;
            }
            let t = self.target.as_ref().unwrap();
            t.Resize(&size)?;
            t.SetDpi(96.0 * scale, 96.0 * scale);
            if self.bitmap.is_none() {
                if let Some(image) = self.data.thumbnail.as_ref().filter(|im| {
                    im.width > 0
                        && im.width <= 320
                        && im.height > 0
                        && im.height <= 180
                        && im.pixels.len() == (im.width * im.height * 4) as usize
                }) {
                    self.bitmap = t
                        .CreateBitmap(
                            D2D_SIZE_U {
                                width: image.width,
                                height: image.height,
                            },
                            Some(image.pixels.as_ptr().cast()),
                            image.width * 4,
                            &D2D1_BITMAP_PROPERTIES {
                                pixelFormat: D2D1_PIXEL_FORMAT {
                                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                                },
                                dpiX: 96.0,
                                dpiY: 96.0,
                            },
                        )
                        .ok();
                }
            }
            let p = palette(self.dark, false);
            let brush = t.CreateSolidColorBrush(&color(p.text, 1.0), None)?;
            t.BeginDraw();
            t.Clear(Some(&color(p.toolbar, 1.0)));
            let text = |value: &str, format: &IDWriteTextFormat, area: D2D_RECT_F, rgb: u32| {
                brush.SetColor(&color(rgb, 1.0));
                t.DrawText(
                    &value.encode_utf16().collect::<Vec<_>>(),
                    format,
                    &area,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            };
            text(
                &self.data.title,
                &self.bold,
                rect(16.0, 12.0, 288.0, 40.0),
                p.text,
            );
            text(
                &self.data.site,
                &self.small,
                rect(16.0, 55.0, 288.0, 19.0),
                p.muted,
            );
            let footer = HEADER_HEIGHT + IMAGE_HEIGHT;
            text("\u{e9d9}", &self.icon, rect(16.0, footer + 11.0, 18.0, 22.0), p.muted);
            text(
                &self.data.memory,
                &self.small,
                rect(42.0, footer + 11.0, 262.0, 22.0),
                p.text,
            );
            text(&self.data.note,&self.small,rect(16.0,footer+35.0,288.0,18.0),p.muted);
            brush.SetColor(&color(p.omni, 1.0));
            t.FillRectangle(&rect(0.0, HEADER_HEIGHT, CARD_WIDTH, IMAGE_HEIGHT), &brush);
            if let Some(bitmap) = &self.bitmap {
                let image_size=bitmap.GetPixelSize();
                let factor=(320.0/image_size.width as f32).min(180.0/image_size.height as f32);
                let (width,height)=(image_size.width as f32*factor,image_size.height as f32*factor);
                t.DrawBitmap(
                    bitmap,
                    Some(&rect((CARD_WIDTH-width)/2.0,HEADER_HEIGHT+(IMAGE_HEIGHT-height)/2.0,width,height)),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
            } else {
                text(
                    &self.data.status,
                    &self.small,
                    rect(18.0, HEADER_HEIGHT + 60.0, 284.0, 60.0),
                    p.muted,
                );
            }
            brush.SetColor(&color(p.sep, 0.65));
            t.DrawLine(
                windows_numerics::Vector2 { X: 0.0, Y: footer + 0.5 },
                windows_numerics::Vector2 { X: CARD_WIDTH, Y: footer + 0.5 },
                &brush,
                1.0,
                None,
            );
            if let Err(e) = t.EndDraw(None, None) {
                self.target = None;
                self.bitmap = None;
                return Err(e);
            }
            Ok(())
        }
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            (*(cs.lpCreateParams as *const RefCell<Inner>))
                .borrow_mut()
                .hwnd = hwnd;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<Inner>;
        if msg == WM_DESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            return LRESULT(0);
        }
        if msg == WM_NCHITTEST {
            return LRESULT(HTTRANSPARENT as isize);
        }
        if msg == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if msg == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            if let Some(cell) = ptr.as_ref() {
                if let Ok(mut i) = cell.try_borrow_mut() {
                    let _ = i.paint();
                }
            }
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
