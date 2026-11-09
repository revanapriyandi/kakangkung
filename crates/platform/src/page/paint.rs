//! Drawing a page with Direct2D: header and search box, navigation, cards with their rows and
//! controls, a slim scrollbar and the message pill at the bottom.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1HwndRenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{IDWriteFactory, IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows_numerics::{Matrix3x2, Vector2};

use super::input::Target;
use super::layout::{Measure, RowBox, HEADER_H, TOGGLE_H};
use super::model::{ButtonStyle, Control};
use super::{Fonts, Inner, ENTER_MS, HOVER_MS, PRESS_MS, TOAST_IN_MS, TOAST_OUT_MS, TOGGLE_MS};
use crate::motion;
use crate::frame::layout::rect;
use crate::frame::scale_of;
use crate::frame::style::{color, glyph};

/// Pages bigger than this (physical pixels) are drawn with the GPU.
const GPU_ABOVE_PIXELS: u64 = 2_200_000;

/// The page's colours (Chrome's settings look in Kakangkung's purple).
pub(super) struct Colors {
    pub bg: u32,
    pub card: u32,
    pub text: u32,
    pub muted: u32,
    pub line: u32,
    pub brand: u32,
    pub on_brand: u32,
    pub selected: u32,
    pub hover: (u32, f32),
    pub field: u32,
    pub danger: u32,
    pub off: u32,
}

pub(super) fn colors(dark: bool) -> Colors {
    if dark {
        Colors { bg: 0x1c1a21, card: 0x2a2830, text: 0xebe8f2, muted: 0xb5b2bd, line: 0x3a3842, brand: 0xa78bfa, on_brand: 0x1c1a21, selected: 0x3f3456, hover: (0xffffff, 0.07), field: 0x24222a, danger: 0xf2b8b5, off: 0x5a5763 }
    } else {
        Colors { bg: 0xf6f4fa, card: 0xffffff, text: 0x1f1d24, muted: 0x5f5c66, line: 0xeceaf1, brand: 0x7c3aed, on_brand: 0xffffff, selected: 0xebe3fd, hover: (0x1f1f1f, 0.05), field: 0xf1eef8, danger: 0xb3261e, off: 0xc9c5d0 }
    }
}

/// DirectWrite measurements for the layout.
pub(super) struct Meter<'a> {
    pub dw: &'a IDWriteFactory,
    pub fonts: &'a Fonts,
}

fn metrics(dw: &IDWriteFactory, text: &str, format: &IDWriteTextFormat, width: f32) -> DWRITE_TEXT_METRICS {
    let w: Vec<u16> = text.encode_utf16().collect();
    let mut m = DWRITE_TEXT_METRICS::default();
    // SAFETY: plain DirectWrite calls with a valid string and out-param.
    unsafe {
        if let Ok(l) = dw.CreateTextLayout(&w, format, width, 10_000.0) {
            let _ = l.GetMetrics(&mut m);
        }
    }
    m
}

impl Measure for Meter<'_> {
    fn width(&self, text: &str) -> f32 {
        metrics(self.dw, text, &self.fonts.button, 10_000.0).widthIncludingTrailingWhitespace.ceil()
    }

    fn height(&self, text: &str, width: f32) -> f32 {
        metrics(self.dw, text, &self.fonts.sub, width).height.ceil()
    }
}

fn rounded(r: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }
}

fn inset(r: D2D_RECT_F, by: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left + by, top: r.top + by, right: r.right - by, bottom: r.bottom - by }
}

/// Drawing state of one frame.
pub(super) struct Pen<'a> {
    t: &'a ID2D1HwndRenderTarget,
    b: &'a ID2D1SolidColorBrush,
    v: &'a crate::icons::Icons,
    pub c: Colors,
}

impl Pen<'_> {
    /// Draw the loaded icon `key` (`platform::favicon`) into `r`; false when it is not loaded.
    // ponytail: a bitmap per draw (a few tiny ones per frame); keep them per target if pages get icon-heavy.
    pub fn icon(&self, r: D2D_RECT_F, key: &str) -> bool {
        let Some(px) = crate::favicon::get(key) else { return false };
        let size = crate::favicon::SIZE;
        let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES {
            pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        // SAFETY: `px` holds `size` rows of `size * 4` bytes; drawing between BeginDraw and EndDraw.
        unsafe {
            let size_u = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width: size, height: size };
            let Ok(b) = self.t.CreateBitmap(size_u, Some(px.as_ptr().cast()), size * 4, &props) else { return false };
            self.t.DrawBitmap(&b, Some(&r), 1.0, windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
        true
    }

    pub fn fill(&self, r: D2D_RECT_F, radius: f32, rgb: u32, a: f32) {
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            self.b.SetColor(&color(rgb, a));
            if radius > 0.0 { self.t.FillRoundedRectangle(&rounded(r, radius), self.b) } else { self.t.FillRectangle(&r, self.b) }
        }
    }

    pub fn outline(&self, r: D2D_RECT_F, radius: f32, rgb: u32, width: f32) {
        // SAFETY: as above.
        unsafe {
            self.b.SetColor(&color(rgb, 1.0));
            self.t.DrawRoundedRectangle(&rounded(inset(r, width / 2.0), radius), self.b, width, None);
        }
    }

    /// A filled circle at `alpha`.
    pub fn dot(&self, x: f32, y: f32, radius: f32, rgb: u32, alpha: f32) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: radius, radiusY: radius };
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            self.b.SetColor(&color(rgb, alpha));
            self.t.FillEllipse(&e, self.b);
        }
    }

    /// A circle outline at `alpha`.
    pub fn ring(&self, x: f32, y: f32, radius: f32, rgb: u32, alpha: f32, width: f32) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: radius, radiusY: radius };
        // SAFETY: as above.
        unsafe {
            self.b.SetColor(&color(rgb, alpha));
            self.t.DrawEllipse(&e, self.b, width, None);
        }
    }

    pub fn circle(&self, x: f32, y: f32, radius: f32, rgb: u32, stroke: Option<f32>) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: radius, radiusY: radius };
        // SAFETY: as above.
        unsafe {
            self.b.SetColor(&color(rgb, 1.0));
            match stroke {
                Some(w) => self.t.DrawEllipse(&e, self.b, w, None),
                None => self.t.FillEllipse(&e, self.b),
            }
        }
    }

    /// An ellipse filled with a diagonal gradient from `from` (top left) to `to` (bottom right).
    pub fn gradient_oval(&self, x: f32, y: f32, rx: f32, ry: f32, from: u32, to: u32) {
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(from, 1.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(to, 1.0) }];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: x - rx, Y: y - ry }, endPoint: Vector2 { X: x + rx, Y: y + ry } };
        // SAFETY: brush creation and drawing on a live render target between BeginDraw and EndDraw.
        unsafe {
            let Ok(collection) = self.t.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            if let Ok(brush) = self.t.CreateLinearGradientBrush(&props, None, &collection) {
                self.t.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: rx, radiusY: ry }, &brush);
            }
        }
    }

    /// A filled ellipse at `alpha`.
    pub fn oval(&self, x: f32, y: f32, rx: f32, ry: f32, rgb: u32, alpha: f32) {
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            self.b.SetColor(&color(rgb, alpha));
            self.t.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: rx, radiusY: ry }, self.b);
        }
    }

    pub fn text(&self, s: &str, r: D2D_RECT_F, f: &IDWriteTextFormat, rgb: u32) {
        self.text_alpha(s, r, f, rgb, 1.0);
    }

    pub fn text_alpha(&self, s: &str, r: D2D_RECT_F, f: &IDWriteTextFormat, rgb: u32, alpha: f32) {
        let w: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: as above.
        unsafe {
            self.b.SetColor(&color(rgb, alpha));
            // An icon-font glyph is drawn as Kakangkung's own line icon where there is one.
            if self.v.draw_glyph(self.t, self.b, s, r, f.GetFontSize()) {
                return;
            }
            self.t.DrawText(&w, f, &r, self.b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }
}

impl Inner {
    /// How far a hover highlight has faded in.
    fn hover_in(&self) -> f32 {
        motion::eased(self.hover_at, 0.0, HOVER_MS)
    }

    /// Strength (0..=1) of `t`'s hover highlight, fading from where it was when the pointer moved.
    pub(super) fn hover_level(&self, t: Target) -> f32 {
        let e = self.hover_in();
        let (now, was) = self.hover_from;
        if self.hover == Some(t) { now + (1.0 - now) * e } else if self.hover_prev == Some(t) { was * (1.0 - e) } else { 0.0 }
    }

    /// Strength (0..=1, a little more just after a press) of `t`'s hover highlight: it fades in
    /// on the new part as it fades out on the one the pointer left.
    pub(super) fn lit(&self, t: Target) -> f32 {
        let level = self.hover_level(t);
        let flash = self.press.filter(|(p, _)| *p == t).map_or(0.0, |(_, at)| 1.0 - motion::eased(at, 0.0, PRESS_MS));
        level + flash * 0.8
    }

    pub(super) fn paint(&mut self) -> windows::core::Result<()> {
        if std::mem::take(&mut self.enter_pending) {
            self.enter = std::time::Instant::now();
        }
        let scale = scale_of(self.hwnd);
        let mut rc = RECT::default();
        // SAFETY: Direct2D drawing on our own window between BeginDraw and EndDraw.
        unsafe {
            GetClientRect(self.hwnd, &mut rc)?;
            let size = D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 };
            if self.rt.is_none() {
                // Drawn by the CPU on ordinary screens: a whole page takes about 5–8 ms at
                // 1600×900, and no graphics device (≈14 MB) is kept for it. Very large pages use
                // the GPU so smooth scrolling stays smooth.
                // Simplification: one pixel-count threshold; a shared D3D device would serve both.
                let kind = if size.width as u64 * size.height as u64 > GPU_ABOVE_PIXELS {
                    D2D1_RENDER_TARGET_TYPE_DEFAULT
                } else {
                    D2D1_RENDER_TARGET_TYPE_SOFTWARE
                };
                let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: kind, dpiX: 96.0 * scale, dpiY: 96.0 * scale, ..Default::default() };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
                let t = self.factory.CreateHwndRenderTarget(&props, &hp)?;
                t.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                self.brush = Some(t.CreateSolidColorBrush(&color(0, 1.0), None)?);
                self.rt = Some(t);
            }
            let (Some(t), Some(b)) = (self.rt.clone(), self.brush.clone()) else { return Ok(()) };
            t.Resize(&size)?;
            t.SetDpi(96.0 * scale, 96.0 * scale);
            t.BeginDraw();
            let pen = Pen { t: &t, b: &b, v: &self.vector, c: colors(self.dark) };
            self.draw(&pen, size.width as f32 / scale, size.height as f32 / scale);
            if t.EndDraw(None, None).is_err() {
                self.rt = None;
            }
        }
        Ok(())
    }

    fn draw(&self, p: &Pen, w: f32, h: f32) {
        let c = &p.c;
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            p.t.Clear(Some(&color(c.bg, 1.0)));
            p.t.PushAxisAlignedClip(&rect(0.0, HEADER_H, w, h - HEADER_H), D2D1_ANTIALIAS_MODE_ALIASED);
            // Coming in: the content rises a little and fades in from the background.
            let come = motion::eased(self.enter, 0.0, ENTER_MS);
            p.t.SetTransform(&Matrix3x2::translation(0.0, HEADER_H - self.scroll + (1.0 - come) * 16.0));
            self.draw_content(p, h);
            p.t.SetTransform(&Matrix3x2::identity());
            if come < 1.0 {
                p.fill(rect(0.0, HEADER_H, w, h - HEADER_H), 0.0, c.bg, 1.0 - come);
            }
            p.t.PopAxisAlignedClip();
        }
        if !self.doc.start {
            self.draw_header(p, w);
        }
        if self.layout.nav {
            self.draw_nav(p);
        }
        self.draw_scrollbar(p, w, h);
        if !self.toast.is_empty() {
            // The message rises in from below and fades; leaving, it fades and sinks a little.
            let rise = motion::eased(self.toast_at, 0.0, TOAST_IN_MS);
            let out = self.toast_out.map_or(0.0, |at| motion::eased(at, 0.0, TOAST_OUT_MS));
            let a = rise * (1.0 - out);
            let m = Meter { dw: &self.dw, fonts: &self.fonts };
            let tw = m.width(&self.toast) + 40.0;
            let y = h - 64.0 + (1.0 - rise) * 14.0 + out * 6.0;
            let r = rect((w - tw) / 2.0, y, tw, 40.0);
            p.fill(D2D_RECT_F { top: r.top + 4.0, bottom: r.bottom + 4.0, ..r }, 20.0, 0x000000, 0.18 * a);
            p.fill(r, 20.0, c.text, a);
            p.text_alpha(&self.toast, r, &self.fonts.button, c.bg, a);
        }
    }

    fn draw_header(&self, p: &Pen, w: f32) {
        let c = &p.c;
        p.fill(rect(0.0, 0.0, w, HEADER_H), 0.0, c.bg, 1.0);
        if self.scroll > 0.5 && !self.doc.start {
            p.fill(rect(0.0, HEADER_H - 1.0, w, 1.0), 0.0, c.line, 1.0);
        }
        if self.layout.nav {
            p.text(&self.doc.title, rect(28.0, 0.0, 220.0, HEADER_H), &self.fonts.header, c.text);
        }
        if !self.doc.search.is_empty() {
            let r = self.layout.search_box();
            // SAFETY: plain focus query.
            let focused = unsafe { GetFocus() } == self.search;
            p.fill(r, 20.0, c.field, 1.0);
            // A hairline that firms up on hover; the brand ring only while typing.
            if focused {
                p.outline(r, 20.0, c.brand, 2.0);
            } else {
                let hover = self.lit(Target::Search).min(1.0);
                if hover > 0.001 {
                    p.outline(r, 20.0, motion::mix(c.field, c.muted, 0.45 * hover), 1.0);
                }
            }
            p.text(glyph::SEARCH, rect(r.left + 10.0, r.top, 28.0, 40.0), &self.fonts.icons, c.muted);
            if self.filter.is_empty() {
                p.text(&self.doc.search, rect(r.left + 46.0, r.top, r.right - r.left - 60.0, 40.0), &self.fonts.title, c.muted);
            }
        }
    }

    fn draw_nav(&self, p: &Pen) {
        let c = &p.c;
        let current = if self.doc.paged { self.filter.is_empty().then_some(self.section.as_str()) } else { Some(self.doc.nav_on.as_str()) };
        for (k, item) in self.doc.nav.iter().enumerate() {
            let r = self.layout.nav_item(k);
            let on = current == Some(item.id.as_str());
            if on {
                p.fill(r, 20.0, c.selected, 1.0);
            } else {
                let a = self.lit(Target::Nav(k));
                if a > 0.001 {
                    p.fill(r, 20.0, c.hover.0, c.hover.1 * a);
                }
            }
            let fg = if on { c.brand } else { c.text };
            p.text(&item.icon, rect(r.left + 12.0, r.top, 24.0, r.bottom - r.top), &self.fonts.icons, fg);
            p.text(&item.label, rect(r.left + 48.0, r.top, r.right - r.left - 56.0, r.bottom - r.top), if on { &self.fonts.nav_on } else { &self.fonts.nav }, fg);
        }
    }

    fn draw_scrollbar(&self, p: &Pen, w: f32, h: f32) {
        let max = self.max_scroll();
        if max <= 0.0 || !(self.mouse_in || self.bar_drag.is_some()) {
            return;
        }
        let wide = self.bar_hover || self.bar_drag.is_some();
        let bw = if wide { 8.0 } else { 4.0 };
        let (top, track) = self.bar_track(h);
        let thumb = (track * track / (track + max)).max(32.0);
        let y = top + (track - thumb) * (self.scroll / max);
        p.fill(rect(w - bw - 3.0, y, bw, thumb), bw / 2.0, p.c.muted, if wide { 0.6 } else { 0.4 });
    }

    /// Content in scrolled coordinates (the transform is set by `draw`).
    fn draw_content(&self, p: &Pen, h: f32) {
        let c = &p.c;
        let (from, to) = (self.scroll - 40.0, self.scroll + h);
        if self.doc.start {
            self.draw_start(p);
        }
        for &(r, s) in &self.layout.titles {
            if r.bottom > from && r.top < to {
                p.text(&self.doc.sections[s].title, r, &self.fonts.section, c.text);
            }
        }
        for &card in &self.layout.cards {
            if card.bottom > from && card.top < to {
                // A soft two-step shadow, the card, and a hairline edge.
                p.fill(D2D_RECT_F { top: card.top + 3.0, bottom: card.bottom + 3.0, ..card }, 12.0, 0x000000, if self.dark { 0.12 } else { 0.025 });
                p.fill(D2D_RECT_F { top: card.top + 1.0, bottom: card.bottom + 1.0, ..card }, 12.0, 0x000000, if self.dark { 0.22 } else { 0.05 });
                p.fill(card, 12.0, c.card, 1.0);
                p.outline(card, 12.0, c.line, 1.0);
            }
        }
        for (k, b) in self.layout.rows.iter().enumerate() {
            if b.rect.bottom > from && b.rect.top < to {
                if b.tile {
                    self.draw_tile(p, k, b);
                } else {
                    self.draw_row(p, k, b);
                }
            }
        }
    }

    fn draw_row(&self, p: &Pen, k: usize, b: &RowBox) {
        let c = &p.c;
        let f = &self.fonts;
        let row = &self.doc.sections[b.section].cards[b.card][b.row];
        let r = b.rect;
        if b.divider {
            p.fill(rect(r.left + 20.0, r.top, r.right - r.left - 40.0, 1.0), 0.0, c.line, 1.0);
        }
        let row_lit = self.lit(Target::Row(k));
        if row_lit > 0.001 {
            p.fill(inset(r, 4.0), 8.0, c.hover.0, c.hover.1 * row_lit);
        }
        let t = b.text;
        if let Control::Illustration { glyph } = &row.control {
            // Empty state: the picture, then the title and text centred under it.
            let come = motion::progress(self.enter, 40.0, 340.0);
            super::art::illustration(p, f, r, glyph, come, self.dark);
            let top = r.top + super::art::ART_H + 8.0;
            p.text(&row.title, rect(r.left + 60.0, top, r.right - r.left - 120.0, 28.0), &f.title_center, c.text);
            p.text(&row.sub, rect(r.left + 60.0, top + 32.0, r.right - r.left - 120.0, r.bottom - top - 40.0), &f.hero_text, c.muted);
            return;
        }
        if let Control::Heading = row.control {
            p.text(&row.title.to_uppercase(), D2D_RECT_F { top: t.bottom - 28.0, ..t }, &f.heading, c.muted);
            return;
        }
        // Title and secondary text, centred as a block.
        let m = Meter { dw: &self.dw, fonts: f };
        let sub_h = if row.sub.is_empty() { 0.0 } else { m.height(&row.sub, t.right - t.left) + 2.0 };
        let top = t.top + ((t.bottom - t.top) - 20.0 - sub_h) / 2.0;
        p.text(&row.title, rect(t.left, top, t.right - t.left, 20.0), &f.title, c.text);
        if sub_h > 0.0 {
            p.text(&row.sub, rect(t.left, top + 22.0, t.right - t.left, sub_h), &f.sub, c.muted);
        }
        let mid = (r.top + r.bottom) / 2.0;
        match &row.control {
            Control::Radio { on, .. } => {
                let x = t.left - 24.0;
                p.circle(x, mid, 9.0, if *on { c.brand } else { c.muted }, Some(2.0));
                if *on {
                    p.circle(x, mid, 5.0, c.brand, None);
                }
            }
            Control::Avatar { initial } => {
                let x = t.left - 36.0;
                p.circle(x, mid, 20.0, if initial.is_empty() { c.off } else { c.brand }, None);
                p.text(if initial.is_empty() { "?" } else { initial }, rect(x - 20.0, mid - 20.0, 40.0, 40.0), &f.button, c.on_brand);
            }
            Control::AvatarImage { initial, path } => {
                let x = t.left - 36.0;
                let target = rect(x - 20.0, mid - 20.0, 40.0, 40.0);
                let key = crate::favicon::load_avatar_file(std::path::Path::new(path));
                if !key.is_some_and(|key| p.icon(target, &key)) {
                    p.circle(x, mid, 20.0, if initial.is_empty() { c.off } else { c.brand }, None);
                    p.text(if initial.is_empty() { "?" } else { initial }, target, &f.button, c.on_brand);
                }
            }
            Control::Toggle { id, on } => {
                // A switch just flipped slides from where it was.
                let t = self.toggles.iter().find(|(t, _)| t == id).map_or(1.0, |(_, at)| motion::eased(*at, 0.0, TOGGLE_MS));
                let pos = if *on { t } else { 1.0 - t };
                let tr = b.control[0];
                p.fill(tr, TOGGLE_H / 2.0, motion::mix(c.off, c.brand, pos), 1.0);
                let kx = tr.left + 10.0 + (tr.right - tr.left - 20.0) * pos;
                // The knob casts a faint shadow on the track.
                p.dot(kx, mid + 0.75, 7.5, 0x000000, 0.2);
                p.circle(kx, mid, 7.0, motion::mix(0xffffff, c.on_brand, pos), None);
            }
            Control::Select { value, options, .. } => {
                let s = b.control[0];
                p.fill(s, 8.0, c.field, 1.0);
                p.outline(s, 8.0, motion::mix(c.line, c.brand, self.lit(Target::Part(k, 0)).min(1.0)), 1.0);
                let label = options.iter().find(|(v, _)| v == value).map_or(value.as_str(), |(_, l)| l.as_str());
                p.text(label, rect(s.left + 12.0, s.top, s.right - s.left - 40.0, s.bottom - s.top), &f.title, c.text);
                p.text(glyph::DOWN, rect(s.right - 30.0, s.top, 20.0, s.bottom - s.top), &f.icons_small, c.muted);
            }
            Control::Buttons(buttons) => {
                for (n, button) in buttons.iter().enumerate() {
                    self.draw_button(p, b.control[n], &button.label, button.style, self.lit(Target::Part(k, n)));
                }
            }
            Control::Text { id, button, .. } => {
                let field = b.control[0];
                let focused = self.edits.iter().any(|(e, h)| e == id && *h == unsafe { GetFocus() });
                p.fill(field, 8.0, c.field, 1.0);
                p.outline(field, 8.0, if focused { c.brand } else { c.line }, if focused { 1.5 } else { 1.0 });
                if let Control::Text { placeholder, value, .. } = &row.control {
                    if value.is_empty() && !focused {
                        p.text(placeholder, rect(field.left + 12.0, field.top, field.right - field.left - 20.0, field.bottom - field.top), &f.title, c.muted);
                    }
                }
                if let Some(&r) = b.control.get(1) {
                    self.draw_button(p, r, button, ButtonStyle::Normal, self.lit(Target::Part(k, 1)));
                }
            }
            Control::Link { .. } => {
                let l = b.control[0];
                p.text("\u{E76C}", l, &f.icons_small, c.muted);
            }
            Control::Value(text) => p.text(text, b.control[0], &f.button, c.text),
            Control::Entry { icon, .. } => {
                p.text(icon, rect(t.left - 30.0, mid - 12.0, 24.0, 24.0), &f.icons, c.muted);
                let x = b.control[0];
                let a = self.lit(Target::Part(k, 0));
                if a > 0.001 {
                    p.fill(x, 16.0, c.hover.0, c.hover.1 * 1.6 * a);
                }
                p.text(glyph::CLOSE, x, &f.icons_small, c.muted);
            }
            Control::None | Control::Heading | Control::Illustration { .. } => {}
        }
    }

    fn draw_button(&self, p: &Pen, r: D2D_RECT_F, label: &str, style: ButtonStyle, hover: f32) {
        let c = &p.c;
        let fg = match style {
            ButtonStyle::Primary => {
                p.fill(r, 16.0, c.brand, 1.0);
                c.on_brand
            }
            ButtonStyle::Danger => {
                p.outline(r, 16.0, c.line, 1.0);
                c.danger
            }
            ButtonStyle::Normal => {
                p.outline(r, 16.0, c.line, 1.0);
                c.brand
            }
        };
        if hover > 0.001 {
            // Filled buttons lighten (dark text on brand would vanish under a dark tint).
            let tint = if matches!(style, ButtonStyle::Primary) { 0xffffff } else { c.hover.0 };
            p.fill(r, 16.0, tint, if matches!(style, ButtonStyle::Primary) { 0.14 } else { c.hover.1 * 1.5 } * hover);
        }
        p.text(label, r, &self.fonts.button, fg);
    }
}
