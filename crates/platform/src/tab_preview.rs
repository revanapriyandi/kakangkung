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
use windows_numerics::Matrix3x2;
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
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
/// Margin of the snapshot's rounded frame from the card's sides (and above the footer).
const IMAGE_INSET: f32 = 10.0;
/// Fade-in of the card: its timer, and how long it takes (ms at normal speed).
const FADE_TIMER: usize = 1;
const FADE_MS: f32 = 120.0;
/// Fade-out of the card when it closes: its timer and length.
const CLOSE_TIMER: usize = 2;
const CLOSE_MS: f32 = 90.0;
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
    /// Centred small text (the reason a snapshot is missing).
    center: IDWriteTextFormat,
    /// When the card appeared: it fades in from there.
    shown_at: Option<std::time::Instant>,
    /// Closing: when, and its opacity (0..=1) then.
    closing: Option<(std::time::Instant, f32)>,
    /// Kakangkung's own line icons.
    vector: Option<crate::icons::Icons>,
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
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let raw = Box::into_raw(Box::new(RefCell::new(Inner {
                hwnd: HWND::default(),
                data: TabPreviewData::default(),
                dark: false,
                shown_at: None,
                closing: None,
                vector: Some(crate::icons::Icons::new(&factory)),
                factory,
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
                center: { let f = text_format(&dw, w!("Segoe UI"), 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?; f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?; f },
            })));
            let hwnd = match CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
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
            // A card that was hidden fades in; one already shown (moving between tabs) just
            // changes its content.
            // Hovered again while fading out: it fades back in from its current opacity.
            let cell = (GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>).as_ref();
            let resumed = cell.and_then(|c| c.borrow_mut().closing.take()).map(|(at, from)| from * (1.0 - crate::motion::fade(at, CLOSE_MS)));
            if resumed.is_some() {
                let _ = KillTimer(Some(self.hwnd), CLOSE_TIMER);
            }
            if !IsWindowVisible(self.hwnd).as_bool() || resumed.is_some() {
                let fading = crate::motion::enabled();
                let from = resumed.unwrap_or(0.0).max(crate::motion::FADE_FROM);
                let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), if fading { (from * 255.0).round() as u8 } else { 255 }, LWA_ALPHA);
                if fading {
                    if let Some(cell) = cell {
                        cell.borrow_mut().shown_at = Some(crate::motion::fade_start_at(from, FADE_MS));
                    }
                    SetTimer(Some(self.hwnd), FADE_TIMER, crate::motion::FRAME_MS, None);
                }
            }
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            Ok(())
        }
    }
    /// Close the card: it fades out quickly, then hides and lets go of its snapshot and drawing
    /// surface. It counts as closed at once.
    pub fn hide(&self) {
        unsafe {
            let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const RefCell<Inner>;
            let fade = IsWindowVisible(self.hwnd).as_bool() && crate::motion::enabled();
            if let (true, Some(Ok(mut i))) = (fade, ptr.as_ref().map(RefCell::try_borrow_mut)) {
                if i.closing.is_none() {
                    let level = i.shown_at.map_or(1.0, |at| crate::motion::fade(at, FADE_MS));
                    i.closing = Some((std::time::Instant::now(), level));
                    let _ = KillTimer(Some(self.hwnd), FADE_TIMER);
                    SetTimer(Some(self.hwnd), CLOSE_TIMER, crate::motion::FRAME_MS, None);
                }
                return;
            }
            close_now(self.hwnd);
        }
    }
    /// Shown and not on its way out.
    pub fn is_visible(&self) -> bool {
        let ptr = unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) } as *const RefCell<Inner>;
        let closing = unsafe { ptr.as_ref() }.and_then(|c| c.try_borrow().ok().map(|i| i.closing.is_some())).unwrap_or(false);
        unsafe { IsWindowVisible(self.hwnd).as_bool() && !closing }
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
            brush.SetColor(&color(p.muted, 1.0));
            if !self.vector.as_ref().is_some_and(|v| v.draw(t, &brush, "\u{E9D9}", rect(16.0, footer + 11.0, 18.0, 22.0), 16.0, 1.5)) {
                text("\u{e9d9}", &self.icon, rect(16.0, footer + 11.0, 18.0, 22.0), p.muted);
            }
            text(
                &self.data.memory,
                &self.small,
                rect(42.0, footer + 11.0, 262.0, 22.0),
                p.text,
            );
            text(&self.data.note,&self.small,rect(16.0,footer+35.0,288.0,18.0),p.muted);
            // The page snapshot sits in a rounded frame inset from the card's edges.
            let frame = rect(IMAGE_INSET, HEADER_HEIGHT, CARD_WIDTH - 2.0 * IMAGE_INSET, IMAGE_HEIGHT - IMAGE_INSET);
            let rounded = D2D1_ROUNDED_RECT { rect: frame, radiusX: 8.0, radiusY: 8.0 };
            brush.SetColor(&color(p.omni, 1.0));
            t.FillRoundedRectangle(&rounded, &brush);
            if let Some(bitmap) = &self.bitmap {
                let image_size = bitmap.GetPixelSize();
                let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
                // Fill the frame (cropping the overflow), like Chrome's hover cards.
                let factor = (fw / image_size.width as f32).max(fh / image_size.height as f32);
                let (width, height) = (image_size.width as f32 * factor, image_size.height as f32 * factor);
                let clip = self.factory.CreateRoundedRectangleGeometry(&rounded).ok();
                if let Some(clip) = &clip {
                    let params = D2D1_LAYER_PARAMETERS { contentBounds: frame, geometricMask: std::mem::ManuallyDrop::new(Some(clip.clone().into())), maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, maskTransform: Matrix3x2::identity(), opacity: 1.0, opacityBrush: std::mem::ManuallyDrop::new(None), layerOptions: D2D1_LAYER_OPTIONS_NONE };
                    t.PushLayer(&params, None);
                }
                t.DrawBitmap(
                    bitmap,
                    Some(&rect(frame.left + (fw - width) / 2.0, frame.top, width, height)),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
                if clip.is_some() {
                    t.PopLayer();
                }
            } else {
                // No snapshot yet: a faint page icon over the reason, centred in the frame.
                let mid = (frame.left + frame.right) / 2.0;
                brush.SetColor(&color(p.muted, 0.55));
                let _ = self.vector.as_ref().is_some_and(|v| v.draw(t, &brush, "\u{E737}", rect(mid - 16.0, frame.top + 46.0, 32.0, 32.0), 30.0, 1.6));
                text(&self.data.status, &self.center, rect(frame.left + 12.0, frame.top + 90.0, frame.right - frame.left - 24.0, 40.0), p.muted);
            }
            // A hairline round the snapshot keeps light pages apart from the card.
            brush.SetColor(&color(if self.dark { 0xffffff } else { 0x000000 }, if self.dark { 0.08 } else { 0.10 }));
            t.DrawRoundedRectangle(&rounded, &brush, 1.0, None);

            if let Err(e) = t.EndDraw(None, None) {
                self.target = None;
                self.bitmap = None;
                return Err(e);
            }
            Ok(())
        }
    }
}
/// Hide the card now and let go of its snapshot and drawing surface.
fn close_now(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Inner>;
        if let Some(Ok(mut i)) = ptr.as_ref().map(RefCell::try_borrow_mut) {
            i.closing = None;
            i.data = TabPreviewData::default();
            i.bitmap = None;
            i.target = None;
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
        if msg == WM_TIMER && wp.0 == CLOSE_TIMER {
            let closing = ptr.as_ref().and_then(|cell| cell.try_borrow().ok().and_then(|i| i.closing));
            let Some((at, from)) = closing else {
                let _ = KillTimer(Some(hwnd), CLOSE_TIMER);
                return LRESULT(0);
            };
            let t = crate::motion::fade(at, CLOSE_MS);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (from * (1.0 - t) * 255.0).round() as u8, LWA_ALPHA);
            if t >= 1.0 {
                let _ = KillTimer(Some(hwnd), CLOSE_TIMER);
                close_now(hwnd);
            }
            return LRESULT(0);
        }
        if msg == WM_TIMER && wp.0 == FADE_TIMER {
            let at = ptr.as_ref().and_then(|cell| cell.try_borrow().ok().and_then(|i| i.shown_at));
            let t = at.map_or(1.0, |at| crate::motion::fade(at, FADE_MS));
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (t * 255.0).round() as u8, LWA_ALPHA);
            if t >= 1.0 {
                let _ = KillTimer(Some(hwnd), FADE_TIMER);
            }
            return LRESULT(0);
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
